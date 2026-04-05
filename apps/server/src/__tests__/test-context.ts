import type { BunSQLiteDatabase } from 'drizzle-orm/bun-sqlite';
import { getTestDb } from './mock-db';

const testsBaseUrl = 'http://localhost:9999';

const tdb = new Proxy({} as BunSQLiteDatabase, {
  get(_target, prop) {
    return getTestDb()[prop as keyof BunSQLiteDatabase];
  },
  set(_target, prop, value) {
    return Reflect.set(getTestDb() as object, prop, value);
  }
});

export { tdb, testsBaseUrl };
