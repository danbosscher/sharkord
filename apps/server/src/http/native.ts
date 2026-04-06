import {
  ServerEvents,
  type TConnectionParams,
  type TNativeEventEnvelope,
  type TNativeEventName
} from '@sharkord/shared';
import { TRPCError } from '@trpc/server';
import type { CreateWSSContextFnOptions } from '@trpc/server/adapters/ws';
import http from 'http';
import z from 'zod';
import { getSettings } from '../db/queries/server';
import { shouldAskServerPassword } from '../helpers/should-ask-server-password';
import { appRouter } from '../routers';
import { getServerBootstrap } from '../services/get-server-bootstrap';
import { createContext } from '../utils/wss';
import { getJsonBody } from './helpers';

const zBootstrapBody = z.object({
  password: z.string().optional()
});

const zListMessagesBody = z.object({
  channelId: z.number(),
  cursor: z.number().nullish(),
  targetMessageId: z.number().nullish(),
  limit: z.number().default(50)
});

const zGetMessageBody = z.object({
  messageId: z.number()
});

const zGetThreadMessagesBody = z.object({
  parentMessageId: z.number(),
  cursor: z.number().nullish(),
  limit: z.number().default(50)
});

const zSendMessageBody = z.object({
  content: z.string(),
  channelId: z.number(),
  files: z.array(z.string()).default([]),
  parentMessageId: z.number().optional(),
  replyToMessageId: z.number().optional()
});

const zEditMessageBody = z.object({
  messageId: z.number(),
  content: z.string()
});

const zDeleteMessageBody = z.object({
  messageId: z.number()
});

const zSignalTypingBody = z.object({
  channelId: z.number(),
  parentMessageId: z.number().optional()
});

const zDeleteTemporaryFileBody = z.object({
  fileId: z.string()
});

const zSearchMessagesBody = z.object({
  query: z.string()
});

const userScopedEventTopics = [
  ServerEvents.NEW_MESSAGE,
  ServerEvents.MESSAGE_UPDATE,
  ServerEvents.MESSAGE_DELETE,
  ServerEvents.MESSAGE_TYPING,
  ServerEvents.THREAD_REPLY_COUNT_UPDATE,
  ServerEvents.CHANNEL_CREATE,
  ServerEvents.CHANNEL_UPDATE,
  ServerEvents.CHANNEL_DELETE,
  ServerEvents.CHANNEL_PERMISSIONS_UPDATE,
  ServerEvents.CHANNEL_READ_STATES_UPDATE,
  ServerEvents.CHANNEL_READ_STATES_DELTA,
  ServerEvents.DM_CONVERSATION_OPEN
] as const satisfies readonly TNativeEventName[];

const globalEventTopics = [
  ServerEvents.USER_JOIN,
  ServerEvents.USER_LEAVE,
  ServerEvents.USER_CREATE,
  ServerEvents.USER_UPDATE,
  ServerEvents.USER_DELETE,
  ServerEvents.SERVER_SETTINGS_UPDATE,
  ServerEvents.CATEGORY_CREATE,
  ServerEvents.CATEGORY_UPDATE,
  ServerEvents.CATEGORY_DELETE,
  ServerEvents.EMOJI_CREATE,
  ServerEvents.EMOJI_UPDATE,
  ServerEvents.EMOJI_DELETE,
  ServerEvents.ROLE_CREATE,
  ServerEvents.ROLE_UPDATE,
  ServerEvents.ROLE_DELETE,
  ServerEvents.PLUGIN_COMMANDS_CHANGE,
  ServerEvents.PLUGIN_COMPONENTS_CHANGE,
  ServerEvents.PLUGIN_METADATA_CHANGE
] as const satisfies readonly TNativeEventName[];

const getAuthorizationToken = (
  req: http.IncomingMessage
): string | undefined => {
  const authHeader = req.headers.authorization;

  if (typeof authHeader === 'string' && authHeader.startsWith('Bearer ')) {
    return authHeader.slice('Bearer '.length).trim();
  }

  const rawHeader = req.headers['x-sharkord-token'];

  if (typeof rawHeader === 'string') {
    return rawHeader.trim();
  }

  return undefined;
};

const writeJson = (
  res: http.ServerResponse,
  statusCode: number,
  body: unknown
) => {
  res.writeHead(statusCode, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
};

const getHttpStatusFromTrpcCode = (
  code: TRPCError['code'] | undefined
): number => {
  switch (code) {
    case 'BAD_REQUEST':
      return 400;
    case 'UNAUTHORIZED':
      return 401;
    case 'FORBIDDEN':
      return 403;
    case 'NOT_FOUND':
      return 404;
    case 'TOO_MANY_REQUESTS':
      return 429;
    default:
      return 500;
  }
};

const writeError = (res: http.ServerResponse, error: unknown) => {
  if (error instanceof z.ZodError) {
    const errors: Record<string, string> = {};

    for (const issue of error.issues) {
      const field = issue.path[0];

      if (typeof field === 'string') {
        errors[field] = issue.message;
      }
    }

    writeJson(res, 400, { errors });
    return;
  }

  if (error instanceof TRPCError) {
    writeJson(res, getHttpStatusFromTrpcCode(error.code), {
      error: error.message
    });
    return;
  }

  writeJson(res, 500, { error: 'Internal server error' });
};

const createNativeAuthenticatedCaller = async (req: http.IncomingMessage) => {
  const token = getAuthorizationToken(req);
  const protocol = req.headers['x-forwarded-proto'] ?? 'http';
  const host = req.headers.host ?? 'localhost';

  const info = {
    connectionParams: {
      token
    } as TConnectionParams,
    accept: 'application/jsonl',
    type: 'subscription',
    isBatchCall: false,
    calls: [],
    signal: new AbortController().signal,
    url: new URL(`${protocol}://${host}`)
  } satisfies CreateWSSContextFnOptions['info'];

  const ctx = await createContext({
    info,
    req,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    res: undefined as any
  });

  ctx.authenticated = true;
  ctx.handshakeHash = 'native-http';

  return {
    caller: appRouter.createCaller(ctx),
    ctx
  };
};

const nativeBootstrapRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ ctx }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    const parsedBody = zBootstrapBody.parse(body);
    const settings = await getSettings();
    const needsPassword = await shouldAskServerPassword(ctx.user.id, {
      password: settings.password,
      onlyAskForPasswordOnFirstJoin: settings.onlyAskForPasswordOnFirstJoin
    });

    if (needsPassword && parsedBody.password !== settings.password) {
      throw new TRPCError({
        code: 'FORBIDDEN',
        message: 'Invalid password'
      });
    }

    const bootstrap = await getServerBootstrap({
      userId: ctx.user.id,
      getStatusById: ctx.getStatusById
    });

    writeJson(res, 200, bootstrap);
  } catch (error) {
    writeError(res, error);
  }
};

const nativeListMessagesRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    const result = await caller.messages.get(zListMessagesBody.parse(body));

    writeJson(res, 200, result);
  } catch (error) {
    writeError(res, error);
  }
};

const nativeGetMessageRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    const result = await caller.messages.getOne(zGetMessageBody.parse(body));

    writeJson(res, 200, result);
  } catch (error) {
    writeError(res, error);
  }
};

const nativeSendMessageRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    const messageId = await caller.messages.send(zSendMessageBody.parse(body));

    writeJson(res, 200, { messageId });
  } catch (error) {
    writeError(res, error);
  }
};

const nativeGetThreadMessagesRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    const result = await caller.messages.getThread(
      zGetThreadMessagesBody.parse(body)
    );

    writeJson(res, 200, result);
  } catch (error) {
    writeError(res, error);
  }
};

const nativeEditMessageRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    await caller.messages.edit(zEditMessageBody.parse(body));

    writeJson(res, 200, { success: true });
  } catch (error) {
    writeError(res, error);
  }
};

const nativeDeleteMessageRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    await caller.messages.delete(zDeleteMessageBody.parse(body));

    writeJson(res, 200, { success: true });
  } catch (error) {
    writeError(res, error);
  }
};

const nativeDeleteTemporaryFileRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    await caller.files.deleteTemporary(zDeleteTemporaryFileBody.parse(body));

    writeJson(res, 200, { success: true });
  } catch (error) {
    writeError(res, error);
  }
};

const nativeSignalTypingRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    await caller.messages.signalTyping(zSignalTypingBody.parse(body));

    writeJson(res, 200, { success: true });
  } catch (error) {
    writeError(res, error);
  }
};

const nativeSearchMessagesRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  try {
    const [{ caller }, body] = await Promise.all([
      createNativeAuthenticatedCaller(req),
      getJsonBody(req)
    ]);

    const result = await caller.messages.search(
      zSearchMessagesBody.parse(body)
    );

    writeJson(res, 200, result);
  } catch (error) {
    writeError(res, error);
  }
};

const nativeEventsRouteHandler = async (
  req: http.IncomingMessage,
  res: http.ServerResponse
) => {
  let heartbeatTimer: ReturnType<typeof setInterval> | undefined;

  try {
    const { ctx } = await createNativeAuthenticatedCaller(req);
    const subscriptions: Array<{ unsubscribe: () => void }> = [];

    const writeEvent = <TEventName extends TNativeEventName>(
      type: TEventName,
      payload: TNativeEventEnvelope<TEventName>['payload']
    ) => {
      const envelope: TNativeEventEnvelope<TEventName> = {
        type,
        payload
      };

      res.write(`event: ${type}\n`);
      res.write(`data: ${JSON.stringify(envelope)}\n\n`);
    };

    const cleanup = () => {
      if (heartbeatTimer) {
        clearInterval(heartbeatTimer);
      }

      for (const subscription of subscriptions) {
        subscription.unsubscribe();
      }
    };

    req.on('close', cleanup);

    res.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache, no-transform',
      Connection: 'keep-alive',
      'X-Accel-Buffering': 'no'
    });

    if (typeof res.flushHeaders === 'function') {
      res.flushHeaders();
    }

    res.write(': native events connected\n\n');

    for (const topic of userScopedEventTopics) {
      subscriptions.push(
        ctx.pubsub.subscribeFor(ctx.user.id, topic).subscribe({
          next(payload) {
            writeEvent(topic, payload);
          }
        })
      );
    }

    for (const topic of globalEventTopics) {
      subscriptions.push(
        ctx.pubsub.subscribe(topic).subscribe({
          next(payload) {
            writeEvent(topic, payload);
          }
        })
      );
    }

    heartbeatTimer = setInterval(() => {
      res.write(': ping\n\n');
    }, 15_000);
  } catch (error) {
    if (!res.headersSent) {
      writeError(res, error);
      return;
    }

    res.end();
  }
};

export {
  nativeBootstrapRouteHandler,
  nativeDeleteMessageRouteHandler,
  nativeDeleteTemporaryFileRouteHandler,
  nativeEditMessageRouteHandler,
  nativeEventsRouteHandler,
  nativeGetMessageRouteHandler,
  nativeGetThreadMessagesRouteHandler,
  nativeListMessagesRouteHandler,
  nativeSearchMessagesRouteHandler,
  nativeSendMessageRouteHandler,
  nativeSignalTypingRouteHandler
};
