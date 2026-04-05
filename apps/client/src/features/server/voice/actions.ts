import type { TPinnedCard } from '@/components/channel-view/voice/hooks/use-pin-card-controller';
import { store } from '@/features/store';
import { logVoice } from '@/helpers/browser-logger';
import {
  LocalStorageKey,
  setLocalStorageItem,
  setLocalStorageItemAsJSON,
  setLocalStorageItemBool
} from '@/helpers/storage';
import { getTRPCClient } from '@/lib/trpc';
import {
  getTrpcError,
  type TExternalStream,
  type TVoiceUserState
} from '@sharkord/shared';
import type { RtpCapabilities } from 'mediasoup-client/types';
import { toast } from 'sonner';
import {
  setCurrentVoiceChannelId,
  setSelectedChannelId
} from '../channels/actions';
import {
  currentVoiceChannelIdSelector,
  selectedChannelIdSelector
} from '../channels/selectors';
import { serverSliceActions } from '../slice';
import { playSound } from '../sounds/actions';
import { SoundType } from '../types';
import { ownUserIdSelector } from '../users/selectors';
import { ownVoiceStateSelector } from './selectors';

export const addUserToVoiceChannel = (
  userId: number,
  channelId: number,
  voiceState: TVoiceUserState
): void => {
  const state = store.getState();
  const ownUserId = ownUserIdSelector(state);
  const currentChannelId = currentVoiceChannelIdSelector(state);

  store.dispatch(
    serverSliceActions.addUserToVoiceChannel({
      userId,
      channelId,
      state: voiceState
    })
  );

  if (userId !== ownUserId && channelId === currentChannelId) {
    playSound(SoundType.REMOTE_USER_JOINED_VOICE_CHANNEL);
  }
};

export const removeUserFromVoiceChannel = (
  userId: number,
  channelId: number
): void => {
  const state = store.getState();
  const ownUserId = ownUserIdSelector(state);
  const currentChannelId = currentVoiceChannelIdSelector(state);

  store.dispatch(
    serverSliceActions.removeUserFromVoiceChannel({ userId, channelId })
  );

  if (userId !== ownUserId && channelId === currentChannelId) {
    playSound(SoundType.REMOTE_USER_LEFT_VOICE_CHANNEL);
  }
};

export const addExternalStreamToVoiceChannel = (
  channelId: number,
  streamId: number,
  stream: TExternalStream
): void => {
  store.dispatch(
    serverSliceActions.addExternalStreamToChannel({
      channelId,
      streamId,
      stream
    })
  );
};

export const updateExternalStreamInVoiceChannel = (
  channelId: number,
  streamId: number,
  stream: TExternalStream
): void => {
  store.dispatch(
    serverSliceActions.updateExternalStreamInChannel({
      channelId,
      streamId,
      stream
    })
  );
};

export const removeExternalStreamFromVoiceChannel = (
  channelId: number,
  streamId: number
): void => {
  store.dispatch(
    serverSliceActions.removeExternalStreamFromChannel({
      channelId,
      streamId
    })
  );
};

export const updateVoiceUserState = (
  userId: number,
  channelId: number,
  newState: Partial<TVoiceUserState>
): void => {
  const state = store.getState();
  const ownUserId = ownUserIdSelector(state);
  const currentChannelId = currentVoiceChannelIdSelector(state);

  if (userId !== ownUserId && channelId === currentChannelId) {
    const currentUserState = state.server.voiceMap[channelId]?.users[userId];

    if (newState.sharingScreen === true && !currentUserState?.sharingScreen) {
      playSound(SoundType.REMOTE_USER_STARTED_SCREENSHARE);
    } else if (
      newState.sharingScreen === false &&
      currentUserState?.sharingScreen
    ) {
      playSound(SoundType.REMOTE_USER_STOPPED_SCREENSHARE);
    }
  }

  store.dispatch(
    serverSliceActions.updateVoiceUserState({ userId, channelId, newState })
  );
};

export const updateOwnVoiceState = (
  newState: Partial<TVoiceUserState>
): void => {
  store.dispatch(serverSliceActions.updateOwnVoiceState(newState));
};

export const joinVoice = async (
  channelId: number,
  options?: {
    force?: boolean;
    suppressErrorToast?: boolean;
  }
): Promise<RtpCapabilities | undefined> => {
  const state = store.getState();
  const currentChannelId = currentVoiceChannelIdSelector(state);
  const force = options?.force ?? false;
  const suppressErrorToast = options?.suppressErrorToast ?? false;

  if (channelId === currentChannelId && !force) {
    // already in the desired channel
    return undefined;
  }

  if (currentChannelId && channelId !== currentChannelId) {
    // is already in a voice channel, leave it first
    await leaveVoice({ reason: 'switch_channel' });
  }

  const { micMuted, soundMuted } = ownVoiceStateSelector(state);
  const client = getTRPCClient();

  try {
    const { routerRtpCapabilities } = await client.voice.join.mutate({
      channelId,
      state: { micMuted, soundMuted }
    });

    setCurrentVoiceChannelId(channelId);

    return routerRtpCapabilities;
  } catch (error) {
    if (!suppressErrorToast) {
      toast.error(
        getTrpcError(
          error,
          force
            ? 'Failed to reconnect voice channel'
            : 'Failed to join voice channel'
        )
      );
    }
  }

  return undefined;
};

export type TLeaveVoiceReason =
  | 'user_disconnect_button'
  | 'switch_channel'
  | 'unknown';

export const leaveVoice = async (options?: {
  reason?: TLeaveVoiceReason;
}): Promise<void> => {
  const state = store.getState();
  const currentChannelId = currentVoiceChannelIdSelector(state);
  const selectedChannelId = selectedChannelIdSelector(state);
  const reason = options?.reason ?? 'unknown';

  if (!currentChannelId) {
    logVoice('Leave voice requested without active channel', { reason });
    return;
  }

  logVoice('Leave voice requested', {
    reason,
    channelId: currentChannelId,
    selectedChannelId
  });

  if (selectedChannelId === currentChannelId) {
    setSelectedChannelId(undefined);
  }

  setCurrentVoiceChannelId(undefined);
  updateOwnVoiceState({ webcamEnabled: false, sharingScreen: false });
  setPinnedCard(undefined);

  const client = getTRPCClient();

  try {
    await client.voice.leave.mutate();
    playSound(SoundType.OWN_USER_LEFT_VOICE_CHANNEL);
  } catch (error) {
    toast.error(getTrpcError(error, 'Failed to leave voice channel'));
  }
};

export const setPinnedCard = (pinnedCard: TPinnedCard | undefined): void => {
  store.dispatch(serverSliceActions.setPinnedCard(pinnedCard));
};

export const setHideNonVideoParticipants = (value: boolean): void => {
  store.dispatch(serverSliceActions.setHideNonVideoParticipants(value));

  try {
    setLocalStorageItem(
      LocalStorageKey.HIDE_NON_VIDEO_PARTICIPANTS,
      String(value)
    );
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};

export const setHideIncomingVideoStreams = (value: boolean): void => {
  store.dispatch(serverSliceActions.setHideIncomingVideoStreams(value));

  try {
    setLocalStorageItemBool(LocalStorageKey.HIDE_INCOMING_VIDEO_STREAMS, value);
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};

const toggleNumberInList = (items: number[], value: number): number[] => {
  return items.includes(value)
    ? items.filter((item) => item !== value)
    : [...items, value];
};

export const toggleEnabledIncomingVideoUserId = (userId: number): void => {
  const current = store.getState().server.enabledIncomingVideoUserIds;
  const next = toggleNumberInList(current, userId);

  store.dispatch(serverSliceActions.setEnabledIncomingVideoUserIds(next));

  try {
    setLocalStorageItemAsJSON(LocalStorageKey.ENABLED_INCOMING_VIDEO_USER_IDS, next);
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};

export const toggleEnabledIncomingScreenShareUserId = (
  userId: number
): void => {
  const current = store.getState().server.enabledIncomingScreenShareUserIds;
  const next = toggleNumberInList(current, userId);

  store.dispatch(serverSliceActions.setEnabledIncomingScreenShareUserIds(next));

  try {
    setLocalStorageItemAsJSON(
      LocalStorageKey.ENABLED_INCOMING_SCREEN_SHARE_USER_IDS,
      next
    );
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};

export const toggleEnabledIncomingExternalVideoStreamId = (
  streamId: number
): void => {
  const current = store.getState().server.enabledIncomingExternalVideoStreamIds;
  const next = toggleNumberInList(current, streamId);

  store.dispatch(
    serverSliceActions.setEnabledIncomingExternalVideoStreamIds(next)
  );

  try {
    setLocalStorageItemAsJSON(
      LocalStorageKey.ENABLED_INCOMING_EXTERNAL_VIDEO_STREAM_IDS,
      next
    );
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};

export const setShowUserBannersInVoice = (value: boolean): void => {
  store.dispatch(serverSliceActions.setShowUserBannersInVoice(value));

  try {
    setLocalStorageItemBool(
      LocalStorageKey.VOICE_CHAT_SHOW_USER_BANNERS,
      value
    );
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};

export const setHideOwnScreenShare = (value: boolean): void => {
  store.dispatch(serverSliceActions.setHideOwnScreenShare(value));

  try {
    setLocalStorageItemBool(LocalStorageKey.HIDE_OWN_SCREEN_SHARE, value);
  } catch (error) {
    console.error('Failed to save voice options:', error);
  }
};
