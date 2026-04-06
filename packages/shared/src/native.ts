import { ServerEvents } from './events';
import type { TCommandsMapByPlugin, TPluginMetadata } from './plugins';
import type {
  TCategory,
  TChannel,
  TFile,
  TJoinedEmoji,
  TJoinedMessage,
  TJoinedPublicUser,
  TJoinedRole
} from './tables';
import type {
  TChannelUserPermissionsMap,
  TPublicServerSettings,
  TReadStateMap
} from './types';
import type { TExternalStreamsMap, TVoiceMap } from './voice';

export type TNativeBootstrap = {
  categories: TCategory[];
  channels: TChannel[];
  users: TJoinedPublicUser[];
  serverId: string;
  serverName: string;
  ownUserId: number;
  voiceMap: TVoiceMap;
  roles: TJoinedRole[];
  emojis: TJoinedEmoji[];
  publicSettings: TPublicServerSettings;
  channelPermissions: TChannelUserPermissionsMap;
  readStates: TReadStateMap;
  commands: TCommandsMapByPlugin;
  pluginIdsWithComponents: string[];
  pluginsMetadata: TPluginMetadata[];
  externalStreamsMap: TExternalStreamsMap;
};

export type TNativeSearchMessage = TJoinedMessage & {
  channelName: string;
  channelIsDm: boolean;
  plainContent: string;
};

export type TNativeSearchFile = {
  file: TFile;
  messageId: number;
  channelId: number;
  messageContent: string;
  messageCreatedAt: number;
  channelName: string;
  channelIsDm: boolean;
};

export type TNativeSearchResults = {
  messages: TNativeSearchMessage[];
  files: TNativeSearchFile[];
};

export type TNativeEventPayloadMap = {
  [ServerEvents.NEW_MESSAGE]: TJoinedMessage;
  [ServerEvents.MESSAGE_UPDATE]: TJoinedMessage;
  [ServerEvents.MESSAGE_DELETE]: {
    messageId: number;
    channelId: number;
  };
  [ServerEvents.MESSAGE_TYPING]: {
    channelId: number;
    userId: number;
    parentMessageId?: number;
  };
  [ServerEvents.THREAD_REPLY_COUNT_UPDATE]: {
    messageId: number;
    channelId: number;
    replyCount: number;
  };
  [ServerEvents.USER_JOIN]: TJoinedPublicUser;
  [ServerEvents.USER_LEAVE]: number;
  [ServerEvents.USER_CREATE]: TJoinedPublicUser;
  [ServerEvents.USER_UPDATE]: TJoinedPublicUser;
  [ServerEvents.USER_DELETE]: {
    isWipe: boolean;
    userId: number;
    deletedUserId: number;
  };
  [ServerEvents.CHANNEL_CREATE]: TChannel;
  [ServerEvents.CHANNEL_UPDATE]: TChannel;
  [ServerEvents.CHANNEL_DELETE]: number;
  [ServerEvents.CHANNEL_PERMISSIONS_UPDATE]: TChannelUserPermissionsMap;
  [ServerEvents.CHANNEL_READ_STATES_UPDATE]: {
    channelId: number;
    count: number;
  };
  [ServerEvents.CHANNEL_READ_STATES_DELTA]: {
    channelId: number;
    delta: number;
  };
  [ServerEvents.SERVER_SETTINGS_UPDATE]: TPublicServerSettings;
  [ServerEvents.CATEGORY_CREATE]: TCategory;
  [ServerEvents.CATEGORY_UPDATE]: TCategory;
  [ServerEvents.CATEGORY_DELETE]: number;
  [ServerEvents.DM_CONVERSATION_OPEN]: {
    channelId: number;
  };
  [ServerEvents.EMOJI_CREATE]: TJoinedEmoji;
  [ServerEvents.EMOJI_UPDATE]: TJoinedEmoji;
  [ServerEvents.EMOJI_DELETE]: number;
  [ServerEvents.ROLE_CREATE]: TJoinedRole;
  [ServerEvents.ROLE_UPDATE]: TJoinedRole;
  [ServerEvents.ROLE_DELETE]: number;
  [ServerEvents.PLUGIN_COMMANDS_CHANGE]: TCommandsMapByPlugin;
  [ServerEvents.PLUGIN_COMPONENTS_CHANGE]: string[];
  [ServerEvents.PLUGIN_METADATA_CHANGE]: TPluginMetadata[];
};

export type TNativeEventName = keyof TNativeEventPayloadMap;

export type TNativeEventEnvelope<
  TEventName extends TNativeEventName = TNativeEventName
> = {
  type: TEventName;
  payload: TNativeEventPayloadMap[TEventName];
};
