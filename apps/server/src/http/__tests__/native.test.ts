import { describe, expect, test } from 'bun:test';
import { getMockedToken } from '../../__tests__/helpers';
import { tdb, testsBaseUrl } from '../../__tests__/test-context';
import { logins, settings } from '../../db/schema';

const nativeBootstrap = async (token: string, body: unknown) =>
  fetch(`${testsBaseUrl}/native/bootstrap`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${token}`
    },
    body: JSON.stringify(body)
  });

describe('/native/bootstrap', () => {
  test('accepts null password values without rejecting the request body', async () => {
    const token = await getMockedToken(1);

    const response = await nativeBootstrap(token, { password: null });

    expect(response.status).toBe(200);

    const data = (await response.json()) as Record<string, unknown>;

    expect(data).toHaveProperty('serverName', 'Test Server');
    expect(data).toHaveProperty('channels');
    expect(data).toHaveProperty('users');
  });

  test('does not require a server password for users who already joined before', async () => {
    await tdb.update(settings).set({
      password: 'serverpass',
      onlyAskForPasswordOnFirstJoin: true
    });

    await tdb.insert(logins).values({
      userId: 1,
      createdAt: Date.now()
    });

    const token = await getMockedToken(1);
    const response = await nativeBootstrap(token, {});

    expect(response.status).toBe(200);

    const data = (await response.json()) as Record<string, unknown>;

    expect(data).toHaveProperty('serverName', 'Test Server');
  });
});
