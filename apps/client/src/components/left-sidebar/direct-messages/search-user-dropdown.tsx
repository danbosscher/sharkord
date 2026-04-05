import { UserAvatar } from '@/components/user-avatar';
import {
  getDuplicateRenderedNames,
  getUserDisambiguator
} from '@/helpers/get-user-disambiguation';
import {
  AutoFocus,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  IconButton,
  Input
} from '@sharkord/ui';
import { Plus } from 'lucide-react';
import { memo, useMemo } from 'react';
import { useTranslation } from 'react-i18next';

const MAX_USERS = 100;

type TSearchUserDropdownProps = {
  query: string;
  setQuery: (query: string) => void;
  usersToStartDm: { id: number; name: string }[];
  onStartDm: (userId: number) => void;
};

const SearchUserDropdown = memo(
  ({
    query,
    setQuery,
    usersToStartDm,
    onStartDm
  }: TSearchUserDropdownProps) => {
    const { t } = useTranslation('sidebar');
    const { allUsers, extraUsers, duplicateRenderedNames } = useMemo(() => {
      const filtered = usersToStartDm.filter((user) =>
        user.name.toLowerCase().includes(query.toLowerCase())
      );

      return {
        allUsers: filtered.slice(0, MAX_USERS),
        extraUsers: filtered.length - MAX_USERS,
        duplicateRenderedNames: getDuplicateRenderedNames(filtered)
      };
    }, [usersToStartDm, query]);

    return (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <IconButton
            variant="ghost"
            size="sm"
            icon={Plus}
            title={t('startNewConversation')}
          />
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          className="w-64 max-h-80 overflow-auto"
        >
          <div className="p-2">
            <AutoFocus>
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t('searchUser')}
              />
            </AutoFocus>
          </div>
          {allUsers.length === 0 && (
            <div className="px-2 pb-2 text-xs text-muted-foreground">
              {t('noUsersAvailable')}
            </div>
          )}
          {allUsers.map((user) => (
            <DropdownMenuItem key={user.id} onClick={() => onStartDm(user.id)}>
              <div className="flex items-center gap-2 min-w-0">
                <UserAvatar
                  userId={user.id}
                  className="h-5 w-5"
                  showUserPopover
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate">{user.name}</div>
                  {getUserDisambiguator(user, duplicateRenderedNames) && (
                    <div className="truncate text-xs text-muted-foreground">
                      {getUserDisambiguator(user, duplicateRenderedNames)}
                    </div>
                  )}
                </div>
              </div>
            </DropdownMenuItem>
          ))}
          {extraUsers > 0 && (
            <div className="px-2 pb-2 text-xs text-muted-foreground">
              {t('andMore', { count: extraUsers })}
            </div>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    );
  }
);

export { SearchUserDropdown };
