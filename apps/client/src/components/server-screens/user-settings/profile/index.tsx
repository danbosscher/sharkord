import { closeServerScreens } from '@/features/server-screens/actions';
import {
  useDisplayNameCollision,
  useOwnPublicUser
} from '@/features/server/users/hooks';
import { useForm } from '@/hooks/use-form';
import { getTRPCClient } from '@/lib/trpc';
import {
  Alert,
  AlertDescription,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Color,
  Group,
  Input,
  Textarea
} from '@sharkord/ui';
import { Info } from 'lucide-react';
import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { AvatarManager } from './avatar-manager';
import { BannerManager } from './banner-manager';

const Profile = memo(() => {
  const { t } = useTranslation('settings');
  const ownPublicUser = useOwnPublicUser();
  const { setTrpcErrors, r, rr, values } = useForm({
    name: ownPublicUser?.name ?? '',
    bannerColor: ownPublicUser?.bannerColor ?? '#FFFFFF',
    bio: ownPublicUser?.bio ?? ''
  });
  const { hasCollision } = useDisplayNameCollision(values.name, {
    excludeUserId: ownPublicUser?.id
  });

  const onUpdateUser = useCallback(async () => {
    const trpc = getTRPCClient();

    try {
      await trpc.users.update.mutate(values);
      toast.success(t('profileUpdated'));
    } catch (error) {
      setTrpcErrors(error);
    }
  }, [values, setTrpcErrors, t]);

  if (!ownPublicUser) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>{t('profileTitle')}</CardTitle>
        <CardDescription>{t('profileDesc')}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <AvatarManager user={ownPublicUser} />

        <Group label={t('usernameLabel')}>
          <Input placeholder={t('usernamePlaceholder')} {...r('name')} />
        </Group>

        <Alert variant="info" className="py-2">
          <Info className="h-4 w-4" />
          <AlertDescription className="text-xs">
            {hasCollision
              ? 'That profile name is already in use. Duplicate profile names are allowed, and Sharkord will show a #id label in mentions and member lists to disambiguate people.'
              : 'Profile names do not need to be unique. If more than one person uses the same name, Sharkord will show a #id label in mentions and member lists to disambiguate people.'}
          </AlertDescription>
        </Alert>

        <Group label={t('bioLabel')}>
          <Textarea placeholder={t('bioPlaceholder')} {...r('bio')} />
        </Group>

        <Group label={t('bannerColorLabel')}>
          <Color {...rr('bannerColor')} defaultValue="#FFFFFF" />
        </Group>

        <BannerManager user={ownPublicUser} />

        <div className="flex justify-end gap-2 pt-4">
          <Button variant="outline" onClick={closeServerScreens}>
            {t('cancel')}
          </Button>
          <Button onClick={onUpdateUser}>{t('saveChanges')}</Button>
        </div>
      </CardContent>
    </Card>
  );
});

export { Profile };
