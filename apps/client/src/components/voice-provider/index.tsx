import { useCurrentVoiceChannelId } from '@/features/server/channels/hooks';
import { useIsConnected } from '@/features/server/hooks';
import { playSound } from '@/features/server/sounds/actions';
import { SoundType } from '@/features/server/types';
import { ownUserIdSelector } from '@/features/server/users/selectors';
import {
  joinVoice,
  leaveVoice,
  updateOwnVoiceState
} from '@/features/server/voice/actions';
import {
  useEnabledIncomingExternalVideoStreamIds,
  useEnabledIncomingScreenShareUserIds,
  useEnabledIncomingVideoUserIds,
  useHideIncomingVideoStreams,
  useOwnVoiceState
} from '@/features/server/voice/hooks';
import {
  voiceChannelExternalStreamsListSelector,
  voiceChannelStateSelector
} from '@/features/server/voice/selectors';
import type { IRootState } from '@/features/store';
import {
  MICROPHONE_GATE_CLOSE_HOLD_MS,
  MICROPHONE_GATE_DEFAULT_THRESHOLD_DB,
  clampMicrophoneDecibels
} from '@/helpers/audio-gate';
import {
  createNoiseGateWorkletNode,
  getNoiseGateWorkletAvailabilitySnapshot,
  markNoiseGateWorkletUnavailable,
  postNoiseGateWorkletConfig
} from '@/helpers/audio-worklet/noise-gate-worklet';
import { createNsChain } from '@/helpers/audio-worklet/ns-worklet';

import { logVoice } from '@/helpers/browser-logger';
import {
  getRestrictOwnAudioSupport,
  getSuppressLocalAudioPlaybackSupport
} from '@/helpers/get-display-media-support';
import { getResWidthHeight } from '@/helpers/get-res-with-height';
import { useScreenShareSupport } from '@/hooks/use-screen-share-support';
import { getTRPCClient } from '@/lib/trpc';
import { NoiseSuppression, VideoCodec, type TDeviceSettings } from '@/types';
import {
  DEFAULT_BITRATE,
  StreamKind,
  type TVoiceUserState
} from '@sharkord/shared';
import { Device } from 'mediasoup-client';
import type {
  RtpCapabilities,
  RtpCodecCapability
} from 'mediasoup-client/types';
import {
  createContext,
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState
} from 'react';
import { useTranslation } from 'react-i18next';
import { useSelector } from 'react-redux';
import { toast } from 'sonner';
import { useDevices } from '../devices-provider/hooks/use-devices';
import {
  clearVoiceControlsBridge,
  setVoiceControlsBridge
} from './controls-bridge';
import { FloatingPinnedCard } from './floating-pinned-card';
import { useLocalStreams } from './hooks/use-local-streams';
import { useRemoteStreams } from './hooks/use-remote-streams';
import {
  useTransportStats,
  type TransportStatsData
} from './hooks/use-transport-stats';
import { useTransports } from './hooks/use-transports';
import { useVoiceControls } from './hooks/use-voice-controls';
import { useVoiceEvents } from './hooks/use-voice-events';
import { VolumeControlProvider } from './volume-control-context';

type AudioVideoRefs = {
  videoRef: React.RefObject<HTMLVideoElement | null>;
  audioRef: React.RefObject<HTMLAudioElement | null>;
  screenShareRef: React.RefObject<HTMLVideoElement | null>;
  screenShareAudioRef: React.RefObject<HTMLAudioElement | null>;
  externalAudioRef: React.RefObject<HTMLAudioElement | null>;
  externalVideoRef: React.RefObject<HTMLVideoElement | null>;
};

export type { AudioVideoRefs };

enum ConnectionStatus {
  DISCONNECTED = 'disconnected',
  CONNECTING = 'connecting',
  CONNECTED = 'connected',
  FAILED = 'failed'
}

const HIDEABLE_INCOMING_VIDEO_KINDS = [
  StreamKind.VIDEO,
  StreamKind.SCREEN,
  StreamKind.EXTERNAL_VIDEO
];

const getErrorMessage = (error: unknown, fallback: string): string => {
  return error instanceof Error ? error.message : fallback;
};

const isRecoverableScreenShareAudioError = (error: unknown): boolean => {
  if (error instanceof DOMException) {
    return !['AbortError', 'NotAllowedError'].includes(error.name);
  }

  if (!(error instanceof Error)) {
    return false;
  }

  const message = error.message.toLowerCase();

  if (
    message.includes('permission denied') ||
    message.includes('permissions policy') ||
    message.includes('user denied') ||
    message.includes('cancel')
  ) {
    return false;
  }

  return (
    message.includes('audio') &&
    (message.includes('source') ||
      message.includes('capture') ||
      message.includes('unsupported') ||
      message.includes('not support') ||
      message.includes('not supported') ||
      message.includes('failed to start'))
  );
};

export type TVoiceProvider = {
  loading: boolean;
  connectionStatus: ConnectionStatus;
  transportStats: TransportStatsData;
  audioVideoRefsMap: Map<number, AudioVideoRefs>;
  ownVoiceState: TVoiceUserState;
  isScreenShareSupported: boolean;
  getOrCreateRefs: (remoteId: number) => AudioVideoRefs;
  getConsumerCodec: (remoteId: number, kind: StreamKind) => string | undefined;
  init: (
    routerRtpCapabilities: RtpCapabilities,
    channelId: number,
    options?: {
      restoreOwnMediaState?: boolean;
      deviceSettingsOverride?: TDeviceSettings;
    }
  ) => Promise<void>;
} & Pick<
  ReturnType<typeof useLocalStreams>,
  | 'localAudioStream'
  | 'localVideoStream'
  | 'localScreenShareStream'
  | 'localScreenShareAudioStream'
> &
  Pick<
    ReturnType<typeof useRemoteStreams>,
    'remoteUserStreams' | 'externalStreams'
  > &
  ReturnType<typeof useVoiceControls>;

const VoiceProviderContext = createContext<TVoiceProvider>({
  loading: false,
  connectionStatus: ConnectionStatus.DISCONNECTED,
  transportStats: {
    producer: null,
    consumer: null,
    screenShare: null,
    totalBytesReceived: 0,
    totalBytesSent: 0,
    isMonitoring: false,
    currentBitrateReceived: 0,
    currentBitrateSent: 0,
    averageBitrateReceived: 0,
    averageBitrateSent: 0
  },
  audioVideoRefsMap: new Map(),
  isScreenShareSupported: false,
  getOrCreateRefs: () => ({
    videoRef: { current: null },
    audioRef: { current: null },
    screenShareRef: { current: null },
    screenShareAudioRef: { current: null },
    externalAudioRef: { current: null },
    externalVideoRef: { current: null }
  }),
  getConsumerCodec: () => undefined,
  init: () => Promise.resolve(),
  toggleMic: () => Promise.resolve(),
  toggleSound: () => Promise.resolve(),
  toggleWebcam: () => Promise.resolve(),
  toggleScreenShare: () => Promise.resolve(),
  ownVoiceState: {
    micMuted: false,
    soundMuted: false,
    webcamEnabled: false,
    sharingScreen: false
  },
  localAudioStream: undefined,
  localVideoStream: undefined,
  localScreenShareStream: undefined,
  localScreenShareAudioStream: undefined,

  remoteUserStreams: {},
  externalStreams: {}
});

type TVoiceProviderProps = {
  children: React.ReactNode;
};

const VoiceProvider = memo(({ children }: TVoiceProviderProps) => {
  const { t } = useTranslation('settings');
  const [loading, setLoading] = useState(false);
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>(
    ConnectionStatus.DISCONNECTED
  );
  const currentVoiceChannelId = useCurrentVoiceChannelId();
  const isServerConnected = useIsConnected();
  const hideIncomingVideoStreams = useHideIncomingVideoStreams();
  const enabledIncomingVideoUserIds = useEnabledIncomingVideoUserIds();
  const enabledIncomingScreenShareUserIds =
    useEnabledIncomingScreenShareUserIds();
  const enabledIncomingExternalVideoStreamIds =
    useEnabledIncomingExternalVideoStreamIds();
  const ownVoiceState = useOwnVoiceState();
  const ownUserId = useSelector((state: IRootState) =>
    ownUserIdSelector(state)
  );
  const currentVoiceChannelState = useSelector((state: IRootState) =>
    currentVoiceChannelId !== undefined
      ? voiceChannelStateSelector(state, currentVoiceChannelId)
      : undefined
  );
  const currentExternalStreams = useSelector((state: IRootState) =>
    currentVoiceChannelId !== undefined
      ? voiceChannelExternalStreamsListSelector(state, currentVoiceChannelId)
      : []
  );
  const routerRtpCapabilities = useRef<RtpCapabilities | null>(null);
  const audioVideoRefsMap = useRef<Map<number, AudioVideoRefs>>(new Map());
  const previousVoiceChannelIdRef = useRef<number | undefined>(undefined);
  const previousServerConnectedRef = useRef(isServerConnected);
  const hadSuccessfulVoiceConnectionRef = useRef(false);
  const attemptedAutoRecoveryRef = useRef(false);
  const recoveringVoiceRef = useRef(false);
  const voiceLifecycleVersionRef = useRef(0);
  const { devices } = useDevices();
  const deviceSettingsRef = useRef(devices);
  const { isScreenShareSupported } = useScreenShareSupport();

  useEffect(() => {
    deviceSettingsRef.current = devices;
  }, [devices]);

  const getOrCreateRefs = useCallback((remoteId: number): AudioVideoRefs => {
    if (!audioVideoRefsMap.current.has(remoteId)) {
      audioVideoRefsMap.current.set(remoteId, {
        videoRef: { current: null },
        audioRef: { current: null },
        screenShareRef: { current: null },
        screenShareAudioRef: { current: null },
        externalAudioRef: { current: null },
        externalVideoRef: { current: null }
      });
    }

    return audioVideoRefsMap.current.get(remoteId)!;
  }, []);

  const {
    addExternalStreamTrack,
    removeExternalStreamTrack,
    removeExternalStream,
    clearExternalStreams,
    addRemoteUserStream,
    removeRemoteUserStream,
    clearRemoteUserStreamsForUser,
    clearRemoteUserStreams,
    externalStreams,
    remoteUserStreams
  } = useRemoteStreams();

  const {
    localAudioProducer,
    localVideoProducer,
    localAudioStream,
    localVideoStream,
    localScreenShareStream,
    localScreenShareAudioStream,
    localScreenShareProducer,
    localScreenShareAudioProducer,
    setLocalAudioStream,
    setLocalVideoStream,
    setLocalScreenShare,
    setLocalScreenShareAudio,
    clearLocalStreams
  } = useLocalStreams();

  const {
    producerTransport,
    consumerTransport,
    createProducerTransport,
    createConsumerTransport,
    consume,
    consumeExistingProducers,
    hasConsumer,
    closeConsumer,
    cleanupTransports,
    getConsumerCodec
  } = useTransports({
    addExternalStreamTrack,
    removeExternalStreamTrack,
    addRemoteUserStream,
    removeRemoteUserStream,
    onTransportFailed: () => {
      setConnectionStatus(ConnectionStatus.FAILED);
    },
    onTransportClosed: () => {
      setConnectionStatus(ConnectionStatus.DISCONNECTED);
    }
  });

  const {
    stats: transportStats,
    startMonitoring,
    stopMonitoring,
    resetStats,
    setScreenShareProducer
  } = useTransportStats();
  const rawMicrophoneStreamRef = useRef<MediaStream | null>(null);
  const transmitMicrophoneTrackRef = useRef<MediaStreamTrack | null>(null);
  const microphoneNoiseGateAudioContextRef = useRef<AudioContext | null>(null);
  const microphoneNoiseGateWorkletNodeRef = useRef<AudioWorkletNode | null>(
    null
  );
  const nsAudioContextsRef = useRef<AudioContext[]>([]);
  const micMutedRef = useRef(ownVoiceState.micMuted);

  const syncOwnVoiceStateWithServer = useCallback(
    async (newState: Partial<TVoiceUserState>) => {
      updateOwnVoiceState(newState);

      if (!currentVoiceChannelId) {
        return;
      }

      try {
        await getTRPCClient().voice.updateState.mutate(newState);
      } catch (error) {
        logVoice('Failed to sync local voice state back to the server', {
          error,
          channelId: currentVoiceChannelId,
          newState
        });
      }
    },
    [currentVoiceChannelId]
  );

  const syncTransmitMicrophoneTrackState = useCallback(() => {
    const track = transmitMicrophoneTrackRef.current;

    if (!track) return;

    const shouldEnable = !micMutedRef.current;

    if (track.enabled !== shouldEnable) {
      track.enabled = shouldEnable;
    }
  }, []);

  const invalidateVoiceLifecycle = useCallback(() => {
    voiceLifecycleVersionRef.current += 1;
    return voiceLifecycleVersionRef.current;
  }, []);

  const isVoiceLifecycleCurrent = useCallback((version: number) => {
    return voiceLifecycleVersionRef.current === version;
  }, []);

  const cleanupMicProcessingResources = useCallback(() => {
    if (microphoneNoiseGateWorkletNodeRef.current) {
      microphoneNoiseGateWorkletNodeRef.current.disconnect();
      microphoneNoiseGateWorkletNodeRef.current = null;
    }

    if (microphoneNoiseGateAudioContextRef.current) {
      microphoneNoiseGateAudioContextRef.current.close();
      microphoneNoiseGateAudioContextRef.current = null;
    }

    nsAudioContextsRef.current.forEach((ctx) => ctx.close());
    nsAudioContextsRef.current = [];

    rawMicrophoneStreamRef.current
      ?.getTracks()
      .forEach((track) => track.stop());
    rawMicrophoneStreamRef.current = null;

    transmitMicrophoneTrackRef.current?.stop();
    transmitMicrophoneTrackRef.current = null;
  }, []);

  useEffect(() => {
    micMutedRef.current = ownVoiceState.micMuted;
    syncTransmitMicrophoneTrackState();
  }, [ownVoiceState.micMuted, syncTransmitMicrophoneTrackState]);

  useEffect(() => {
    if (!microphoneNoiseGateWorkletNodeRef.current) return;

    postNoiseGateWorkletConfig(microphoneNoiseGateWorkletNodeRef.current, {
      enabled: devices.noiseGateEnabled ?? true,
      holdMs: MICROPHONE_GATE_CLOSE_HOLD_MS
    });
  }, [devices.noiseGateEnabled]);

  useEffect(() => {
    if (!microphoneNoiseGateWorkletNodeRef.current) return;

    postNoiseGateWorkletConfig(microphoneNoiseGateWorkletNodeRef.current, {
      thresholdDb: clampMicrophoneDecibels(
        devices.noiseGateThresholdDb ?? MICROPHONE_GATE_DEFAULT_THRESHOLD_DB
      )
    });
  }, [devices.noiseGateThresholdDb]);

  const clearMicStream = useCallback(() => {
    cleanupMicProcessingResources();
    localAudioProducer.current?.close();
    localAudioProducer.current = undefined;
    setLocalAudioStream(undefined);
  }, [cleanupMicProcessingResources, localAudioProducer, setLocalAudioStream]);

  const startMicStream = useCallback(
    async (settingsOverride?: TDeviceSettings) => {
      const targetDevices = settingsOverride ?? deviceSettingsRef.current;

      try {
        logVoice('Starting microphone stream');
        cleanupMicProcessingResources();

        const useNsChain =
          targetDevices.noiseSuppression === NoiseSuppression.DTLN ||
          targetDevices.noiseSuppression === NoiseSuppression.RNNOISE;
        const useStandardNs =
          targetDevices.noiseSuppression === NoiseSuppression.STANDARD;
        const useDtln =
          targetDevices.noiseSuppression === NoiseSuppression.DTLN;

        const hasSpecificMic =
          !!targetDevices.microphoneId &&
          targetDevices.microphoneId !== 'default';

        const micStreamConstraints: MediaStreamConstraints = {
          audio: {
            deviceId: hasSpecificMic
              ? { exact: targetDevices.microphoneId }
              : undefined,
            autoGainControl: targetDevices.autoGainControl,
            echoCancellation: targetDevices.echoCancellation,
            noiseSuppression: useStandardNs,
            sampleRate: useDtln ? 16000 : undefined,
            channelCount: 1
          },
          video: false
        };

        logVoice(
          'Requesting microphone stream with constraints',
          micStreamConstraints
        );

        const rawStream =
          await navigator.mediaDevices.getUserMedia(micStreamConstraints);

        logVoice('Microphone stream obtained', { stream: rawStream });

        const rawAudioTrack = rawStream.getAudioTracks()[0];

        if (rawAudioTrack) {
          const shouldUseNoiseGate = !!targetDevices.noiseGateEnabled;
          const noiseGateAvailability =
            getNoiseGateWorkletAvailabilitySnapshot();
          let transmitTrack: MediaStreamTrack = rawAudioTrack;
          let transmitStream: MediaStream = rawStream;

          if (shouldUseNoiseGate && noiseGateAvailability.available) {
            let audioContext: AudioContext | null = null;

            try {
              audioContext = new window.AudioContext();
              const source = audioContext.createMediaStreamSource(rawStream);
              const noiseGateNode = await createNoiseGateWorkletNode(
                audioContext,
                {
                  enabled: true,
                  thresholdDb: clampMicrophoneDecibels(
                    targetDevices.noiseGateThresholdDb ??
                      MICROPHONE_GATE_DEFAULT_THRESHOLD_DB
                  ),
                  holdMs: MICROPHONE_GATE_CLOSE_HOLD_MS
                }
              );
              const destination = audioContext.createMediaStreamDestination();

              source.connect(noiseGateNode);
              noiseGateNode.connect(destination);

              const processedTrack = destination.stream.getAudioTracks()[0];

              if (processedTrack) {
                rawMicrophoneStreamRef.current = rawStream;
                microphoneNoiseGateAudioContextRef.current = audioContext;
                microphoneNoiseGateWorkletNodeRef.current = noiseGateNode;
                transmitTrack = processedTrack;
                transmitStream = destination.stream;
              } else {
                noiseGateNode.disconnect();
                audioContext.close();
                audioContext = null;
                logVoice(
                  'Noise gate worklet produced no audio track, using ungated mic stream'
                );
              }
            } catch (error) {
              if (audioContext) {
                audioContext.close();
              }

              logVoice(
                'Failed to initialize live noise gate worklet, using ungated mic stream',
                {
                  error
                }
              );
              markNoiseGateWorkletUnavailable(
                'Failed to initialize the noise gate audio processor.'
              );
            }
          } else if (shouldUseNoiseGate && !noiseGateAvailability.available) {
            logVoice(
              'Noise gate unavailable, using ungated microphone stream',
              {
                reason: noiseGateAvailability.reason
              }
            );
          }

          if (useNsChain) {
            logVoice('Setting up noise suppression', {
              type: targetDevices.noiseSuppression
            });

            try {
              const chain = await createNsChain(
                targetDevices.noiseSuppression,
                transmitStream
              );
              nsAudioContextsRef.current = chain.contexts;
              transmitTrack = chain.outputTrack;
              transmitStream = new MediaStream([chain.outputTrack]);
              logVoice('Noise suppression chain ready');
            } catch (nsError) {
              logVoice('Failed to set up noise suppression', {
                error: nsError
              });
            }
          }

          transmitMicrophoneTrackRef.current = transmitTrack;
          setLocalAudioStream(transmitStream);
          syncTransmitMicrophoneTrackState();

          logVoice('Obtained audio track', { audioTrack: rawAudioTrack });

          localAudioProducer.current = await producerTransport.current?.produce(
            {
              track: transmitTrack,
              codecOptions: {
                opusStereo: false,
                opusFec: true,
                opusDtx: false,
                opusMaxPlaybackRate: 48000,
                opusMaxAverageBitrate: 128000
              },
              appData: { kind: StreamKind.AUDIO }
            }
          );

          logVoice('Microphone audio producer created', {
            producer: localAudioProducer.current
          });

          localAudioProducer.current?.on('@close', async () => {
            logVoice('Audio producer closed');

            const trpc = getTRPCClient();

            try {
              await trpc.voice.closeProducer.mutate({
                kind: StreamKind.AUDIO
              });
            } catch (error) {
              logVoice('Error closing audio producer', { error });
            }
          });

          rawAudioTrack.onended = () => {
            logVoice('Audio track ended, cleaning up microphone');

            transmitStream.getAudioTracks().forEach((track) => {
              track.stop();
            });
            cleanupMicProcessingResources();
            localAudioProducer.current?.close();

            setLocalAudioStream(undefined);
          };
        } else {
          rawStream.getTracks().forEach((track) => track.stop());
          throw new Error('Failed to obtain audio track from microphone');
        }
      } catch (error) {
        cleanupMicProcessingResources();
        localAudioProducer.current = undefined;
        setLocalAudioStream(undefined);
        logVoice('Error starting microphone stream', { error });
        throw error;
      }
    },
    [
      cleanupMicProcessingResources,
      producerTransport,
      setLocalAudioStream,
      localAudioProducer,
      syncTransmitMicrophoneTrackState
    ]
  );

  const clearWebcamStream = useCallback(
    (stream?: MediaStream) => {
      const targetStream = stream ?? localVideoStream;

      targetStream?.getVideoTracks().forEach((track) => {
        logVoice('Stopping video track', { track });
        track.stop();
      });

      localVideoProducer.current?.close();
      localVideoProducer.current = undefined;

      setLocalVideoStream(undefined);
    },
    [localVideoProducer, localVideoStream, setLocalVideoStream]
  );

  const startWebcamStream = useCallback(
    async (settingsOverride?: TDeviceSettings) => {
      const targetDevices = settingsOverride ?? deviceSettingsRef.current;

      try {
        logVoice('Starting webcam stream');

        const hasSpecificWebcam =
          !!targetDevices.webcamId && targetDevices.webcamId !== 'default';

        const webcamConstraints: MediaStreamConstraints = {
          video: {
            deviceId: hasSpecificWebcam
              ? { exact: targetDevices.webcamId }
              : undefined,
            frameRate: targetDevices.webcamFramerate,
            ...getResWidthHeight(targetDevices.webcamResolution)
          },
          audio: false
        };

        logVoice(
          'Requesting webcam stream with constraints',
          webcamConstraints
        );

        const stream =
          await navigator.mediaDevices.getUserMedia(webcamConstraints);

        logVoice('Webcam stream obtained', { stream });

        setLocalVideoStream(stream);

        const videoTrack = stream.getVideoTracks()[0];

        if (videoTrack) {
          logVoice('Obtained video track', { videoTrack });

          localVideoProducer.current = await producerTransport.current?.produce(
            {
              track: videoTrack,
              appData: { kind: StreamKind.VIDEO }
            }
          );

          logVoice('Webcam video producer created', {
            producer: localVideoProducer.current
          });

          localVideoProducer.current?.on('@close', async () => {
            logVoice('Video producer closed');

            const trpc = getTRPCClient();

            try {
              await trpc.voice.closeProducer.mutate({
                kind: StreamKind.VIDEO
              });
            } catch (error) {
              logVoice('Error closing video producer', { error });
            }
          });

          videoTrack.onended = () => {
            logVoice('Video track ended, cleaning up webcam');

            clearWebcamStream(stream);
            void syncOwnVoiceStateWithServer({ webcamEnabled: false });
          };
        } else {
          throw new Error('Failed to obtain video track from webcam');
        }
      } catch (error) {
        logVoice('Error starting webcam stream', { error });
        throw error;
      }
    },
    [
      setLocalVideoStream,
      localVideoProducer,
      producerTransport,
      clearWebcamStream,
      syncOwnVoiceStateWithServer
    ]
  );

  const stopWebcamStream = useCallback(() => {
    logVoice('Stopping webcam stream');

    clearWebcamStream();
  }, [clearWebcamStream]);

  const clearScreenShareStream = useCallback(
    (stream?: MediaStream) => {
      const targetStream = stream ?? localScreenShareStream;

      targetStream?.getTracks().forEach((track) => {
        logVoice('Stopping screen share track', { track });
        track.stop();
      });

      localScreenShareProducer.current?.close();
      localScreenShareProducer.current = undefined;

      localScreenShareAudioProducer.current?.close();
      localScreenShareAudioProducer.current = undefined;

      setScreenShareProducer(null);
      setLocalScreenShare(undefined);
      setLocalScreenShareAudio(undefined);
    },
    [
      localScreenShareAudioProducer,
      localScreenShareProducer,
      localScreenShareStream,
      setLocalScreenShare,
      setLocalScreenShareAudio,
      setScreenShareProducer
    ]
  );

  const stopScreenShareStream = useCallback(() => {
    logVoice('Stopping screen share stream');

    clearScreenShareStream();
  }, [clearScreenShareStream]);

  const startScreenShareStream = useCallback(
    async (settingsOverride?: TDeviceSettings) => {
      const targetDevices = settingsOverride ?? deviceSettingsRef.current;
      const canRestrictOwnAudio = getRestrictOwnAudioSupport();
      const canSuppressLocalAudioPlayback =
        getSuppressLocalAudioPlaybackSupport();

      const getDisplayMediaConstraints = (
        includeAudio: boolean
      ): MediaStreamConstraints => ({
        video: {
          ...getResWidthHeight(targetDevices.screenResolution),
          frameRate: targetDevices.screenFramerate
        },
        audio: includeAudio
          ? {
              echoCancellation: false,
              noiseSuppression: false,
              autoGainControl: false,
              // @ts-expect-error - experimental, not in types yet
              suppressLocalAudioPlayback: canSuppressLocalAudioPlayback
                ? (targetDevices.suppressLocalAudioPlayback ?? false)
                : undefined,
              restrictOwnAudio: canRestrictOwnAudio
                ? (targetDevices.restrictOwnAudio ?? false)
                : undefined
            }
          : false
      });

      const requestScreenShareStream = async (
        includeAudio: boolean
      ): Promise<MediaStream> => {
        const constraints = getDisplayMediaConstraints(includeAudio);

        logVoice('Requesting display media with constraints', constraints);

        return navigator.mediaDevices.getDisplayMedia(constraints);
      };

      const shouldRequestAudio = targetDevices.shareSystemAudio !== false;

      try {
        logVoice('Starting screen share stream');

        let stream: MediaStream;

        try {
          stream = await requestScreenShareStream(shouldRequestAudio);
        } catch (error) {
          if (shouldRequestAudio && isRecoverableScreenShareAudioError(error)) {
            logVoice(
              'Retrying screen share without audio after audio capture failure',
              {
                error
              }
            );

            stream = await requestScreenShareStream(false);
            toast.warning(t('screenShareAudioUnavailable'));
          } else {
            throw error;
          }
        }

        logVoice('Screen share stream obtained', { stream });
        setLocalScreenShare(stream);
        setLocalScreenShareAudio(undefined);

        const videoTrack = stream.getVideoTracks()[0];
        const audioTrack = stream.getAudioTracks()[0];

        if (videoTrack) {
          logVoice('Obtained video track', { videoTrack });

          let preferredCodec: RtpCodecCapability | undefined;

          if (
            targetDevices.screenCodec &&
            targetDevices.screenCodec !== VideoCodec.AUTO &&
            routerRtpCapabilities.current?.codecs
          ) {
            preferredCodec = routerRtpCapabilities.current.codecs.find(
              (c) =>
                c.mimeType.toLowerCase() ===
                targetDevices.screenCodec.toLowerCase()
            );

            if (preferredCodec) {
              logVoice('Using preferred screen share codec', {
                codec: preferredCodec.mimeType
              });
            }
          }

          const maxBitrateKbps = targetDevices.screenBitrate ?? DEFAULT_BITRATE;

          localScreenShareProducer.current =
            await producerTransport.current?.produce({
              track: videoTrack,
              codec: preferredCodec,
              codecOptions: {
                videoGoogleStartBitrate: Math.min(2000, maxBitrateKbps),
                videoGoogleMaxBitrate: maxBitrateKbps,
                videoGoogleMinBitrate: Math.min(200, maxBitrateKbps)
              },
              appData: { kind: StreamKind.SCREEN }
            });

          setScreenShareProducer(localScreenShareProducer.current);

          localScreenShareProducer.current?.on('@close', async () => {
            logVoice('Screen share producer closed');

            const trpc = getTRPCClient();

            try {
              await trpc.voice.closeProducer.mutate({
                kind: StreamKind.SCREEN
              });
            } catch (error) {
              logVoice('Error closing screen share producer', { error });
            }
          });

          videoTrack.onended = () => {
            logVoice('Screen share track ended, cleaning up screen share');

            clearScreenShareStream(stream);
            void syncOwnVoiceStateWithServer({ sharingScreen: false });
          };

          if (audioTrack) {
            logVoice('Obtained audio track', { audioTrack });
            setLocalScreenShareAudio(new MediaStream([audioTrack]));

            localScreenShareAudioProducer.current =
              await producerTransport.current?.produce({
                track: audioTrack,
                codecOptions: {
                  opusStereo: true,
                  opusFec: true,
                  opusDtx: false,
                  opusMaxPlaybackRate: 48000,
                  opusMaxAverageBitrate: 128000
                },
                appData: { kind: StreamKind.SCREEN_AUDIO }
              });

            audioTrack.onended = () => {
              localScreenShareAudioProducer.current?.close();
              localScreenShareAudioProducer.current = undefined;
              setLocalScreenShareAudio(undefined);
            };
          }

          return videoTrack;
        } else {
          throw new Error('No video track obtained for screen share');
        }
      } catch (error) {
        logVoice('Error starting screen share stream', { error });
        throw error;
      }
    },
    [
      setLocalScreenShare,
      localScreenShareProducer,
      localScreenShareAudioProducer,
      producerTransport,
      setScreenShareProducer,
      setLocalScreenShareAudio,
      clearScreenShareStream,
      syncOwnVoiceStateWithServer,
      t
    ]
  );

  const restoreOwnMediaState = useCallback(
    async (settingsOverride?: TDeviceSettings) => {
      if (ownVoiceState.webcamEnabled && !localVideoStream) {
        try {
          await startWebcamStream(settingsOverride);
        } catch (error) {
          logVoice('Failed to restore webcam after voice recovery', { error });
          await syncOwnVoiceStateWithServer({ webcamEnabled: false });
        }
      }

      if (ownVoiceState.sharingScreen && !localScreenShareStream) {
        try {
          await startScreenShareStream(settingsOverride);
        } catch (error) {
          logVoice('Failed to restore screen share after voice recovery', {
            error
          });
          await syncOwnVoiceStateWithServer({ sharingScreen: false });
        }
      }
    },
    [
      localScreenShareStream,
      localVideoStream,
      ownVoiceState.sharingScreen,
      ownVoiceState.webcamEnabled,
      startScreenShareStream,
      startWebcamStream,
      syncOwnVoiceStateWithServer
    ]
  );

  const applyLiveDeviceSettings = useCallback(
    async (nextDevices: TDeviceSettings) => {
      const previousDevices = deviceSettingsRef.current;
      const errors: string[] = [];
      const micWasActive = !!(
        localAudioProducer.current ||
        localAudioStream ||
        rawMicrophoneStreamRef.current
      );
      const webcamWasActive = !!(
        ownVoiceState.webcamEnabled ||
        localVideoProducer.current ||
        localVideoStream
      );

      const shouldRefreshMic =
        previousDevices.microphoneId !== nextDevices.microphoneId ||
        previousDevices.autoGainControl !== nextDevices.autoGainControl ||
        previousDevices.echoCancellation !== nextDevices.echoCancellation ||
        previousDevices.noiseSuppression !== nextDevices.noiseSuppression ||
        previousDevices.noiseGateEnabled !== nextDevices.noiseGateEnabled ||
        previousDevices.noiseGateThresholdDb !==
          nextDevices.noiseGateThresholdDb;

      const shouldRefreshWebcam =
        previousDevices.webcamId !== nextDevices.webcamId ||
        previousDevices.webcamResolution !== nextDevices.webcamResolution ||
        previousDevices.webcamFramerate !== nextDevices.webcamFramerate;

      if (shouldRefreshMic && micWasActive) {
        try {
          clearMicStream();
          await startMicStream(nextDevices);

          if (!localAudioProducer.current) {
            throw new Error('Failed to restart microphone stream');
          }
        } catch (error) {
          const message = getErrorMessage(
            error,
            'Failed to restart microphone stream'
          );
          errors.push(message);

          try {
            clearMicStream();
            await startMicStream(previousDevices);
          } catch (restoreError) {
            errors.push(
              `Previous microphone settings could not be restored: ${getErrorMessage(
                restoreError,
                'restore failed'
              )}`
            );
          }
        }
      }

      if (shouldRefreshWebcam && webcamWasActive) {
        try {
          clearWebcamStream();
          await startWebcamStream(nextDevices);
        } catch (error) {
          const message = getErrorMessage(
            error,
            'Failed to restart webcam stream'
          );
          errors.push(message);

          try {
            clearWebcamStream();
            await startWebcamStream(previousDevices);
          } catch (restoreError) {
            errors.push(
              `Previous webcam settings could not be restored: ${getErrorMessage(
                restoreError,
                'restore failed'
              )}`
            );
          }
        }
      }

      if (errors.length > 0) {
        throw new Error(errors.join(' '));
      }
    },
    [
      clearMicStream,
      clearWebcamStream,
      localAudioProducer,
      localAudioStream,
      localVideoProducer,
      localVideoStream,
      ownVoiceState.webcamEnabled,
      startMicStream,
      startWebcamStream
    ]
  );

  const cleanup = useCallback(
    (options?: { invalidateLifecycle?: boolean }) => {
      logVoice('Running voice provider cleanup');

      if (options?.invalidateLifecycle ?? true) {
        invalidateVoiceLifecycle();
      }

      stopMonitoring();
      resetStats();
      cleanupMicProcessingResources();
      clearLocalStreams();
      clearRemoteUserStreams();
      clearExternalStreams();
      cleanupTransports();
      routerRtpCapabilities.current = null;

      setLoading(false);
      setConnectionStatus(ConnectionStatus.DISCONNECTED);
    },
    [
      invalidateVoiceLifecycle,
      stopMonitoring,
      resetStats,
      cleanupMicProcessingResources,
      clearLocalStreams,
      clearRemoteUserStreams,
      clearExternalStreams,
      cleanupTransports
    ]
  );

  const isIncomingVideoKindEnabled = useCallback(
    (kind: StreamKind, remoteId: number) => {
      switch (kind) {
        case StreamKind.VIDEO:
          return enabledIncomingVideoUserIds.includes(remoteId);
        case StreamKind.SCREEN:
          return enabledIncomingScreenShareUserIds.includes(remoteId);
        case StreamKind.EXTERNAL_VIDEO:
          return enabledIncomingExternalVideoStreamIds.includes(remoteId);
        default:
          return true;
      }
    },
    [
      enabledIncomingExternalVideoStreamIds,
      enabledIncomingScreenShareUserIds,
      enabledIncomingVideoUserIds
    ]
  );

  const shouldConsumeIncomingStreamKind = useCallback(
    (kind: StreamKind, remoteId: number) => {
      if (!hideIncomingVideoStreams) {
        return true;
      }

      if (!HIDEABLE_INCOMING_VIDEO_KINDS.includes(kind)) {
        return true;
      }

      return isIncomingVideoKindEnabled(kind, remoteId);
    },
    [hideIncomingVideoStreams, isIncomingVideoKindEnabled]
  );

  const init = useCallback(
    async (
      incomingRouterRtpCapabilities: RtpCapabilities,
      channelId: number,
      options?: {
        restoreOwnMediaState?: boolean;
        deviceSettingsOverride?: TDeviceSettings;
      }
    ) => {
      logVoice('Initializing voice provider', {
        incomingRouterRtpCapabilities,
        channelId
      });

      const lifecycleVersion = invalidateVoiceLifecycle();
      cleanup({ invalidateLifecycle: false });

      try {
        setLoading(true);
        setConnectionStatus(ConnectionStatus.CONNECTING);

        routerRtpCapabilities.current = incomingRouterRtpCapabilities;

        const device = new Device();

        await device.load({
          routerRtpCapabilities: incomingRouterRtpCapabilities
        });

        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice('Aborting stale voice init after device load', {
            channelId,
            lifecycleVersion
          });
          return;
        }

        await createProducerTransport(device);

        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice('Aborting stale voice init after producer transport setup', {
            channelId,
            lifecycleVersion
          });
          cleanup();
          return;
        }

        await createConsumerTransport(device);

        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice('Aborting stale voice init after consumer transport setup', {
            channelId,
            lifecycleVersion
          });
          cleanup();
          return;
        }

        await consumeExistingProducers(incomingRouterRtpCapabilities, {
          shouldConsumeKind: shouldConsumeIncomingStreamKind
        });

        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice(
            'Aborting stale voice init after existing producer consume',
            {
              channelId,
              lifecycleVersion
            }
          );
          cleanup();
          return;
        }

        await startMicStream(options?.deviceSettingsOverride);

        if (!localAudioProducer.current) {
          throw new Error('Failed to initialize microphone stream');
        }

        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice('Aborting stale voice init after microphone setup', {
            channelId,
            lifecycleVersion
          });
          cleanup();
          return;
        }

        if (options?.restoreOwnMediaState) {
          await restoreOwnMediaState(options.deviceSettingsOverride);
        }

        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice('Aborting stale voice init after own media restore', {
            channelId,
            lifecycleVersion
          });
          cleanup();
          return;
        }

        startMonitoring(producerTransport.current, consumerTransport.current);
        setConnectionStatus(ConnectionStatus.CONNECTED);
        setLoading(false);
        playSound(SoundType.OWN_USER_JOINED_VOICE_CHANNEL);
      } catch (error) {
        if (!isVoiceLifecycleCurrent(lifecycleVersion)) {
          logVoice('Ignoring stale voice init failure', {
            channelId,
            lifecycleVersion,
            error
          });
          return;
        }

        logVoice('Error initializing voice provider', { error });

        cleanup();
        setConnectionStatus(ConnectionStatus.FAILED);
        setLoading(false);

        throw error;
      }
    },
    [
      cleanup,
      createProducerTransport,
      createConsumerTransport,
      consumeExistingProducers,
      invalidateVoiceLifecycle,
      isVoiceLifecycleCurrent,
      shouldConsumeIncomingStreamKind,
      startMicStream,
      startMonitoring,
      producerTransport,
      consumerTransport,
      restoreOwnMediaState,
      localAudioProducer
    ]
  );

  const recoverVoiceConnection = useCallback(async () => {
    if (!currentVoiceChannelId || recoveringVoiceRef.current) {
      return;
    }

    recoveringVoiceRef.current = true;
    setConnectionStatus(ConnectionStatus.CONNECTING);
    let joinedVoiceChannel = false;

    try {
      const response = await joinVoice(currentVoiceChannelId, {
        force: true,
        suppressErrorToast: true
      });

      if (!response) {
        setConnectionStatus(ConnectionStatus.FAILED);
        return;
      }

      joinedVoiceChannel = true;
      await init(response, currentVoiceChannelId, {
        restoreOwnMediaState: true
      });
    } catch (error) {
      logVoice('Automatic voice recovery failed', { error });

      if (joinedVoiceChannel) {
        await leaveVoice({ reason: 'unknown' });
      }
    } finally {
      recoveringVoiceRef.current = false;
    }
  }, [currentVoiceChannelId, init]);

  const attemptVoiceRecovery = useCallback(() => {
    if (!currentVoiceChannelId) {
      return;
    }

    const shouldRecover =
      (connectionStatus === ConnectionStatus.FAILED ||
        connectionStatus === ConnectionStatus.DISCONNECTED) &&
      isServerConnected &&
      hadSuccessfulVoiceConnectionRef.current &&
      !attemptedAutoRecoveryRef.current &&
      !loading &&
      !recoveringVoiceRef.current;

    if (!shouldRecover) {
      return;
    }

    attemptedAutoRecoveryRef.current = true;
    void recoverVoiceConnection();
  }, [
    connectionStatus,
    currentVoiceChannelId,
    isServerConnected,
    loading,
    recoverVoiceConnection
  ]);

  const { toggleMic, toggleSound, toggleWebcam, toggleScreenShare } =
    useVoiceControls({
      startMicStream,
      localAudioStream,
      startWebcamStream,
      stopWebcamStream,
      startScreenShareStream,
      stopScreenShareStream
    });

  const setMicMutedForBridge = useCallback(
    async (muted: boolean) => {
      if (ownVoiceState.micMuted === muted) return;
      await toggleMic();
    },
    [ownVoiceState.micMuted, toggleMic]
  );

  const setSoundMutedForBridge = useCallback(
    async (muted: boolean) => {
      if (ownVoiceState.soundMuted === muted) return;
      await toggleSound();
    },
    [ownVoiceState.soundMuted, toggleSound]
  );

  const applyDeviceSettingsForBridge = useCallback(
    async (nextDevices: TDeviceSettings) => {
      if (!currentVoiceChannelId) {
        return;
      }

      await applyLiveDeviceSettings(nextDevices);
    },
    [applyLiveDeviceSettings, currentVoiceChannelId]
  );

  useEffect(() => {
    setVoiceControlsBridge({
      setMicMuted: setMicMutedForBridge,
      setSoundMuted: setSoundMutedForBridge,
      applyDeviceSettings: applyDeviceSettingsForBridge
    });

    return () => {
      clearVoiceControlsBridge();
    };
  }, [
    applyDeviceSettingsForBridge,
    setMicMutedForBridge,
    setSoundMutedForBridge
  ]);

  useVoiceEvents({
    consume,
    shouldConsumeKind: shouldConsumeIncomingStreamKind,
    removeRemoteUserStream,
    removeExternalStreamTrack,
    removeExternalStream,
    clearRemoteUserStreamsForUser,
    rtpCapabilities: routerRtpCapabilities.current!
  });

  useEffect(() => {
    const previousVoiceChannelId = previousVoiceChannelIdRef.current;

    previousVoiceChannelIdRef.current = currentVoiceChannelId;

    if (previousVoiceChannelId !== currentVoiceChannelId) {
      attemptedAutoRecoveryRef.current = false;

      if (previousVoiceChannelId !== currentVoiceChannelId) {
        hadSuccessfulVoiceConnectionRef.current = false;
      }
    }

    if (
      previousVoiceChannelId !== undefined &&
      currentVoiceChannelId === undefined
    ) {
      logVoice('Left voice channel, releasing local voice resources');
      cleanup();
    }
  }, [currentVoiceChannelId, cleanup]);

  useEffect(() => {
    const wasServerConnected = previousServerConnectedRef.current;

    previousServerConnectedRef.current = isServerConnected;

    if (!currentVoiceChannelId) {
      return;
    }

    if (wasServerConnected && !isServerConnected) {
      attemptedAutoRecoveryRef.current = false;

      if (connectionStatus !== ConnectionStatus.DISCONNECTED) {
        setConnectionStatus(ConnectionStatus.DISCONNECTED);
      }
    }
  }, [connectionStatus, currentVoiceChannelId, isServerConnected]);

  useEffect(() => {
    if (
      !currentVoiceChannelId ||
      connectionStatus !== ConnectionStatus.CONNECTED ||
      !routerRtpCapabilities.current
    ) {
      return;
    }

    const rtpCapabilities = routerRtpCapabilities.current;
    const currentUsers = currentVoiceChannelState?.users ?? {};

    Object.entries(currentUsers).forEach(([userIdStr, userState]) => {
      const userId = Number(userIdStr);

      if (userId === ownUserId) {
        return;
      }

      const shouldConsumeVideo =
        userState.webcamEnabled &&
        shouldConsumeIncomingStreamKind(StreamKind.VIDEO, userId);

      if (shouldConsumeVideo) {
        if (!hasConsumer(userId, StreamKind.VIDEO)) {
          void consume(userId, StreamKind.VIDEO, rtpCapabilities);
        }
      } else {
        closeConsumer(userId, StreamKind.VIDEO);
      }

      const shouldConsumeScreen =
        userState.sharingScreen &&
        shouldConsumeIncomingStreamKind(StreamKind.SCREEN, userId);

      if (shouldConsumeScreen) {
        if (!hasConsumer(userId, StreamKind.SCREEN)) {
          void consume(userId, StreamKind.SCREEN, rtpCapabilities);
        }
      } else {
        closeConsumer(userId, StreamKind.SCREEN);
      }

      if (!userState.sharingScreen) {
        closeConsumer(userId, StreamKind.SCREEN_AUDIO);
      }
    });

    currentExternalStreams.forEach((stream) => {
      if (!stream.tracks.video) {
        closeConsumer(stream.streamId, StreamKind.EXTERNAL_VIDEO);
        return;
      }

      const shouldConsumeExternalVideo = shouldConsumeIncomingStreamKind(
        StreamKind.EXTERNAL_VIDEO,
        stream.streamId
      );

      if (shouldConsumeExternalVideo) {
        if (!hasConsumer(stream.streamId, StreamKind.EXTERNAL_VIDEO)) {
          void consume(
            stream.streamId,
            StreamKind.EXTERNAL_VIDEO,
            rtpCapabilities
          );
        }
      } else {
        closeConsumer(stream.streamId, StreamKind.EXTERNAL_VIDEO);
      }
    });
  }, [
    closeConsumer,
    connectionStatus,
    consume,
    currentExternalStreams,
    currentVoiceChannelId,
    currentVoiceChannelState,
    hasConsumer,
    ownUserId,
    shouldConsumeIncomingStreamKind
  ]);

  useEffect(() => {
    if (!currentVoiceChannelId) {
      hadSuccessfulVoiceConnectionRef.current = false;
      attemptedAutoRecoveryRef.current = false;
      recoveringVoiceRef.current = false;
      return;
    }

    if (connectionStatus === ConnectionStatus.CONNECTED) {
      hadSuccessfulVoiceConnectionRef.current = true;
      attemptedAutoRecoveryRef.current = false;
      return;
    }

    attemptVoiceRecovery();
  }, [attemptVoiceRecovery, connectionStatus, currentVoiceChannelId]);

  useEffect(() => {
    const retryVoiceRecovery = () => {
      attemptedAutoRecoveryRef.current = false;
      attemptVoiceRecovery();
    };

    const handleOnline = () => {
      retryVoiceRecovery();
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState !== 'visible') {
        return;
      }

      retryVoiceRecovery();
    };

    window.addEventListener('online', handleOnline);
    document.addEventListener('visibilitychange', handleVisibilityChange);

    return () => {
      window.removeEventListener('online', handleOnline);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [attemptVoiceRecovery]);

  useEffect(() => {
    return () => {
      logVoice('Voice provider unmounting, cleaning up resources');
      cleanup();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const contextValue = useMemo<TVoiceProvider>(
    () => ({
      loading,
      connectionStatus,
      transportStats,
      audioVideoRefsMap: audioVideoRefsMap.current,
      isScreenShareSupported,
      getOrCreateRefs,
      getConsumerCodec,
      init,

      toggleMic,
      toggleSound,
      toggleWebcam,
      toggleScreenShare,
      ownVoiceState,

      localAudioStream,
      localVideoStream,
      localScreenShareStream,
      localScreenShareAudioStream,

      remoteUserStreams,
      externalStreams
    }),
    [
      loading,
      connectionStatus,
      transportStats,
      isScreenShareSupported,
      getOrCreateRefs,
      getConsumerCodec,
      init,

      toggleMic,
      toggleSound,
      toggleWebcam,
      toggleScreenShare,
      ownVoiceState,

      localAudioStream,
      localVideoStream,
      localScreenShareStream,
      localScreenShareAudioStream,
      remoteUserStreams,
      externalStreams
    ]
  );

  return (
    <VoiceProviderContext.Provider value={contextValue}>
      <VolumeControlProvider>
        <div className="relative">
          <FloatingPinnedCard
            remoteUserStreams={remoteUserStreams}
            externalStreams={externalStreams}
            localScreenShareStream={localScreenShareStream}
            localVideoStream={localVideoStream}
          />
          {children}
        </div>
      </VolumeControlProvider>
    </VoiceProviderContext.Provider>
  );
});

export { VoiceProvider, VoiceProviderContext };
