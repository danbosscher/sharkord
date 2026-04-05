import { DELETED_USER_IDENTITY_AND_NAME, Permission } from '@sharkord/shared';
import { eq } from 'drizzle-orm';
import { z } from 'zod';
import { db } from '../../db';
import { publishUser } from '../../db/publishers';
import { users } from '../../db/schema';
import { invariant } from '../../utils/invariant';
import { protectedProcedure } from '../../utils/trpc';

const renameUserRoute = protectedProcedure
  .input(
    z.object({
      userId: z.number().int().positive(),
      name: z
        .string()
        .trim()
        .min(1)
        .max(24)
        .refine((val) => val !== DELETED_USER_IDENTITY_AND_NAME, {
          message: 'Protected username'
        })
    })
  )
  .mutation(async ({ ctx, input }) => {
    await ctx.needsPermission(Permission.MANAGE_USERS);

    const targetUser = await db
      .select({
        id: users.id,
        identity: users.identity
      })
      .from(users)
      .where(eq(users.id, input.userId))
      .get();

    invariant(targetUser, {
      code: 'NOT_FOUND',
      message: 'User not found.'
    });

    invariant(targetUser.identity !== DELETED_USER_IDENTITY_AND_NAME, {
      code: 'BAD_REQUEST',
      message: 'Cannot rename the deleted user placeholder.'
    });

    const updatedUser = await db
      .update(users)
      .set({ name: input.name })
      .where(eq(users.id, input.userId))
      .returning()
      .get();

    invariant(updatedUser, {
      code: 'NOT_FOUND',
      message: 'User not found.'
    });

    await publishUser(updatedUser.id, 'update');
  });

export { renameUserRoute };
