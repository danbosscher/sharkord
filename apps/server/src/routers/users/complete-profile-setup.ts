import { DELETED_USER_IDENTITY_AND_NAME } from '@sharkord/shared';
import { eq } from 'drizzle-orm';
import { z } from 'zod';
import { db } from '../../db';
import { publishUser } from '../../db/publishers';
import { users } from '../../db/schema';
import { invariant } from '../../utils/invariant';
import { protectedProcedure } from '../../utils/trpc';

const completeProfileSetupRoute = protectedProcedure
  .input(
    z.object({
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
    const updatedUser = await db
      .update(users)
      .set({
        name: input.name,
        profileSetupCompleted: true,
        updatedAt: Date.now()
      })
      .where(eq(users.id, ctx.userId))
      .returning()
      .get();

    invariant(updatedUser, {
      code: 'NOT_FOUND',
      message: 'User not found'
    });

    await publishUser(updatedUser.id, 'update');

    return updatedUser;
  });

export { completeProfileSetupRoute };
