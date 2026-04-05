import { getRenderedUsername } from './get-rendered-username';

type TUserLike = {
  id: number;
  name: string;
};

const getDuplicateRenderedNames = (users: TUserLike[]) => {
  const counts = new Map<string, number>();

  users.forEach((user) => {
    const key = getRenderedUsername(user).trim().toLowerCase();

    counts.set(key, (counts.get(key) ?? 0) + 1);
  });

  return new Set(
    Array.from(counts.entries())
      .filter(([, count]) => count > 1)
      .map(([name]) => name)
  );
};

const getUserDisambiguator = (
  user: TUserLike,
  duplicateRenderedNames: Set<string>
) => {
  const key = getRenderedUsername(user).trim().toLowerCase();

  return duplicateRenderedNames.has(key) ? `#${user.id}` : undefined;
};

export { getDuplicateRenderedNames, getUserDisambiguator };
