import { type TNativeBootstrap, type UserStatus } from '@sharkord/shared';
import { db } from '../db';
import {
  getAllChannelUserPermissions,
  getChannelsForUser,
  getChannelsReadStatesForUser
} from '../db/queries/channels';
import { getEmojis } from '../db/queries/emojis';
import { getRoles } from '../db/queries/roles';
import { getPublicSettings, getSettings } from '../db/queries/server';
import { getPublicUsers } from '../db/queries/users';
import { categories } from '../db/schema';
import { pluginManager } from '../plugins';
import { VoiceRuntime } from '../runtimes/voice';
import { invariant } from '../utils/invariant';

type GetServerBootstrapOptions = {
  userId: number;
  getStatusById: (userId: number) => UserStatus;
};

const getServerBootstrap = async ({
  userId,
  getStatusById
}: GetServerBootstrapOptions): Promise<TNativeBootstrap> => {
  const [
    allCategories,
    channelsForUser,
    publicUsers,
    roles,
    emojis,
    channelPermissions,
    readStates,
    publicSettings,
    pluginsMetadata,
    settings
  ] = await Promise.all([
    db.select().from(categories),
    getChannelsForUser(userId),
    getPublicUsers(true),
    getRoles(),
    getEmojis(),
    getAllChannelUserPermissions(userId),
    getChannelsReadStatesForUser(userId),
    getPublicSettings(),
    pluginManager.getActivePluginMetadata(),
    getSettings()
  ]);

  const processedPublicUsers = publicUsers.map((user) => ({
    ...user,
    status: getStatusById(user.id),
    _identity: undefined
  }));

  const ownUser = processedPublicUsers.find((user) => user.id === userId);

  invariant(ownUser, {
    code: 'NOT_FOUND',
    message: 'User not present in public users'
  });

  return {
    categories: allCategories,
    channels: channelsForUser,
    users: processedPublicUsers,
    serverId: settings.serverId,
    serverName: settings.name,
    ownUserId: userId,
    voiceMap: VoiceRuntime.getVoiceMap(),
    roles,
    emojis,
    publicSettings,
    channelPermissions,
    readStates,
    commands: pluginManager.getCommands(),
    pluginIdsWithComponents: pluginManager.getPluginIdsWithComponents(),
    pluginsMetadata,
    externalStreamsMap: VoiceRuntime.getExternalStreamsMap()
  };
};

export { getServerBootstrap };
