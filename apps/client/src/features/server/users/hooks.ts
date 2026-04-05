import { getDuplicateRenderedNames } from '@/helpers/get-user-disambiguation';
import { getRenderedUsername } from '@/helpers/get-rendered-username';
import type { IRootState } from '@/features/store';
import { useMemo } from 'react';
import { useSelector } from 'react-redux';
import {
  filteredUsersSelector,
  isOwnUserSelector,
  ownPublicUserSelector,
  ownUserIdSelector,
  ownUserSelector,
  userByIdSelector,
  usernamesSelector,
  usersSelector,
  userStatusSelector
} from './selectors';

export const useUsers = () => useSelector(usersSelector);

export const useOwnUser = () => useSelector(ownUserSelector);

export const useOwnUserId = () => useSelector(ownUserIdSelector);

export const useIsOwnUser = (userId: number | null) =>
  useSelector((state: IRootState) =>
    userId !== null ? isOwnUserSelector(state, userId) : false
  );

export const useUserById = (userId: number | null) =>
  useSelector((state: IRootState) =>
    userId !== null ? userByIdSelector(state, userId) : undefined
  );

export const useOwnPublicUser = () =>
  useSelector((state: IRootState) => ownPublicUserSelector(state));

export const useUserStatus = (userId: number) =>
  useSelector((state: IRootState) => userStatusSelector(state, userId));

export const useUsernames = () => useSelector(usernamesSelector);

export const useFilteredUsers = () => useSelector(filteredUsersSelector);

export const useDuplicateRenderedNames = () => {
  const users = useUsers();

  return useMemo(() => getDuplicateRenderedNames(users), [users]);
};

export const useDisplayNameCollision = (
  name: string,
  options?: {
    excludeUserId?: number;
  }
) => {
  const users = useUsers();

  return useMemo(() => {
    const normalizedName = getRenderedUsername({ name }).trim().toLowerCase();

    if (!normalizedName) {
      return {
        hasCollision: false,
        matchingUserIds: [] as number[]
      };
    }

    const matchingUserIds = users
      .filter((user) => user.id !== options?.excludeUserId)
      .filter(
        (user) =>
          getRenderedUsername(user).trim().toLowerCase() === normalizedName
      )
      .map((user) => user.id);

    return {
      hasCollision: matchingUserIds.length > 0,
      matchingUserIds
    };
  }, [name, options?.excludeUserId, users]);
};
