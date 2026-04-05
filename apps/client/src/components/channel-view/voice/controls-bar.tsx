import { useChannelCan } from '@/features/server/hooks';
import { leaveVoice } from '@/features/server/voice/actions';
import { setHideIncomingVideoStreams } from '@/features/server/voice/actions';
import {
  useHideIncomingVideoStreams,
  useOwnVoiceState,
  useVoice
} from '@/features/server/voice/hooks';
import { cn } from '@/lib/utils';
import { ChannelPermission } from '@sharkord/shared';
import { Button, Tooltip } from '@sharkord/ui';
import {
  Eye,
  EyeOff,
  HeadphoneOff,
  Headphones,
  Mic,
  MicOff,
  Monitor,
  PhoneOff,
  ScreenShareOff,
  Video,
  VideoOff
} from 'lucide-react';
import { memo, useCallback, useMemo } from 'react';
import { ControlToggleButton } from './control-toggle-button';
import { useControlsBarVisibility } from './hooks/use-controls-bar-visibility';

type TControlsBarProps = {
  channelId: number;
};

const ControlsBar = memo(({ channelId }: TControlsBarProps) => {
  const {
    toggleMic,
    toggleSound,
    toggleWebcam,
    toggleScreenShare,
    isScreenShareSupported
  } = useVoice();
  const ownVoiceState = useOwnVoiceState();
  const hideIncomingVideoStreams = useHideIncomingVideoStreams();
  const channelCan = useChannelCan(channelId);
  const isVisible = useControlsBarVisibility();

  const permissions = useMemo(
    () => ({
      canSpeak: channelCan(ChannelPermission.SPEAK),
      canWebcam: channelCan(ChannelPermission.WEBCAM),
      canShareScreen: channelCan(ChannelPermission.SHARE_SCREEN)
    }),
    [channelCan]
  );

  const handleToggleHideIncomingVideoStreams = useCallback(() => {
    setHideIncomingVideoStreams(!hideIncomingVideoStreams);
  }, [hideIncomingVideoStreams]);

  return (
    <div
      className={cn(
        'absolute bottom-4 left-0 right-0 flex justify-center items-center pointer-events-none px-2 md:bottom-8',
        'transition-all duration-300 ease-in-out gap-2 md:gap-3',
        isVisible ? 'opacity-100 translate-y-0' : 'opacity-0 translate-y-10'
      )}
    >
      <div
        className={cn(
          'flex items-center gap-2 pointer-events-auto',
          'h-12 px-2 rounded-md border shadow-xl md:h-14',
          'bg-card border-border/50 backdrop-blur-md'
        )}
      >
        <ControlToggleButton
          enabled={ownVoiceState.micMuted}
          enabledLabel="Unmute"
          disabledLabel="Mute"
          enabledIcon={MicOff}
          disabledIcon={Mic}
          enabledClassName="bg-red-500/20 text-red-500 hover:bg-red-500/30 hover:text-red-500"
          onClick={toggleMic}
          disabled={!permissions.canSpeak}
        />

        <ControlToggleButton
          enabled={ownVoiceState.soundMuted}
          enabledLabel="Undeafen"
          disabledLabel="Deafen"
          enabledIcon={HeadphoneOff}
          disabledIcon={Headphones}
          enabledClassName="bg-red-500/20 text-red-500 hover:bg-red-500/30 hover:text-red-500"
          onClick={toggleSound}
        />

        <ControlToggleButton
          enabled={hideIncomingVideoStreams}
          enabledLabel="Show Incoming Video"
          disabledLabel="Audio Only"
          enabledIcon={EyeOff}
          disabledIcon={Eye}
          enabledClassName="bg-amber-500/20 text-amber-500 hover:bg-amber-500/30 hover:text-amber-500"
          onClick={handleToggleHideIncomingVideoStreams}
        />

        <ControlToggleButton
          enabled={ownVoiceState.webcamEnabled}
          enabledLabel="Stop Video"
          disabledLabel="Start Video"
          enabledIcon={Video}
          disabledIcon={VideoOff}
          enabledClassName="bg-green-500/20 text-green-500 hover:bg-green-500/30 hover:text-green-500"
          onClick={toggleWebcam}
          disabled={!permissions.canWebcam}
        />

        {isScreenShareSupported && (
          <ControlToggleButton
            enabled={ownVoiceState.sharingScreen}
            enabledLabel="Stop Sharing"
            disabledLabel="Share Screen"
            enabledIcon={ScreenShareOff}
            disabledIcon={Monitor}
            enabledClassName="bg-blue-500/20 text-blue-500 hover:bg-blue-500/30 hover:text-blue-500"
            onClick={toggleScreenShare}
            disabled={!permissions.canShareScreen}
          />
        )}
      </div>

      <Tooltip content="Disconnect">
        <Button
          size="icon"
          className={cn(
            'pointer-events-auto h-12 w-12 rounded-md text-white shadow-xl transition-all active:scale-95 md:h-14 md:w-[4.5rem]',
            'bg-[#ec4245] hover:bg-[#da373c]'
          )}
          onClick={() => leaveVoice()}
          aria-label="Disconnect"
        >
          <PhoneOff size={24} fill="currentColor" />
        </Button>
      </Tooltip>
    </div>
  );
});

export { ControlsBar };
