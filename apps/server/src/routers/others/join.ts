import { ActivityLogType, ServerEvents, UserStatus } from '@sharkord/shared';
import { eq } from 'drizzle-orm';
import { z } from 'zod';
import { db } from '../../db';
import { getSettings } from '../../db/queries/server';
import { users } from '../../db/schema';
import { shouldAskServerPassword } from '../../helpers/should-ask-server-password';
import { logger } from '../../logger';
import { pluginManager } from '../../plugins';
import { eventBus } from '../../plugins/event-bus';
import { enqueueActivityLog } from '../../queues/activity-log';
import { enqueueLogin } from '../../queues/logins';
import { getServerBootstrap } from '../../services/get-server-bootstrap';
import { invariant } from '../../utils/invariant';
import { rateLimitedProcedure, t } from '../../utils/trpc';

const joinServerRoute = rateLimitedProcedure(t.procedure, {
  maxRequests: 5,
  windowMs: 60_000,
  logLabel: 'joinServer'
})
  .input(
    z.object({
      handshakeHash: z.string(),
      password: z.string().optional()
    })
  )
  .query(async ({ input, ctx }) => {
    const connectionInfo = ctx.getConnectionInfo();
    const settings = await getSettings();
    const shouldAskForPassword = await shouldAskServerPassword(ctx.user.id, {
      password: settings.password,
      onlyAskForPasswordOnFirstJoin: settings.onlyAskForPasswordOnFirstJoin
    });

    invariant(
      input.handshakeHash &&
        ctx.handshakeHash &&
        input.handshakeHash === ctx.handshakeHash,
      {
        code: 'FORBIDDEN',
        message: 'Invalid handshake hash'
      }
    );

    invariant(
      shouldAskForPassword ? input.password === settings.password : true,
      {
        code: 'FORBIDDEN',
        message: 'Invalid password'
      }
    );

    invariant(ctx.user, {
      code: 'UNAUTHORIZED',
      message: 'User not authenticated'
    });

    ctx.authenticated = true;
    ctx.setWsUserId(ctx.user.id);

    const bootstrap = await getServerBootstrap({
      userId: ctx.user.id,
      getStatusById: ctx.getStatusById
    });

    const foundPublicUser = bootstrap.users.find((user) => user.id === ctx.user.id);

    invariant(foundPublicUser, {
      code: 'NOT_FOUND',
      message: 'User not present in public users'
    });

    logger.info('%s joined the server', ctx.user.name);

    ctx.pubsub.publish(ServerEvents.USER_JOIN, {
      ...foundPublicUser,
      status: UserStatus.ONLINE
    });

    if (connectionInfo?.ip) {
      ctx.saveUserIp(ctx.user.id, connectionInfo.ip);
    }
    await db
      .update(users)
      .set({ lastLoginAt: Date.now() })
      .where(eq(users.id, ctx.user.id));

    enqueueLogin(ctx.user.id, connectionInfo);
    enqueueActivityLog({
      type: ActivityLogType.USER_JOINED,
      userId: ctx.user.id,
      ip: connectionInfo?.ip
    });

    eventBus.emit('user:joined', {
      userId: ctx.user.id,
      username: ctx.user.name
    });

    return bootstrap;
  });

export { joinServerRoute };
