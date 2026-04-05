import { AvatarManager } from '@/components/server-screens/user-settings/profile/avatar-manager';
import { BannerManager } from '@/components/server-screens/user-settings/profile/banner-manager';
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
  Group,
  Input
} from '@sharkord/ui';
import { Info } from 'lucide-react';
import { memo, useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';

const UserOnboarding = memo(() => {
  const { t } = useTranslation('settings');
  const ownPublicUser = useOwnPublicUser();
  const { values, r, setTrpcErrors } = useForm({
    name: ownPublicUser?.name ?? ''
  });
  const { hasCollision } = useDisplayNameCollision(values.name, {
    excludeUserId: ownPublicUser?.id
  });
  const [saving, setSaving] = useState(false);

  const canContinue = useMemo(
    () => values.name.trim().length > 0 && !saving,
    [values.name, saving]
  );

  const finishSetup = useCallback(async () => {
    const trpc = getTRPCClient();

    setSaving(true);

    try {
      await trpc.users.completeProfileSetup.mutate({
        name: values.name
      });
    } catch (error) {
      setTrpcErrors(error);
      toast.error('Could not save your profile setup. Please try again.');
    } finally {
      setSaving(false);
    }
  }, [setTrpcErrors, values.name]);

  if (!ownPublicUser || ownPublicUser.profileSetupCompleted) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 px-4 py-6 backdrop-blur-sm">
      <Card className="w-full max-w-3xl border-border/60 shadow-2xl">
        <CardHeader>
          <CardTitle>{t('profileTitle')}</CardTitle>
          <CardDescription>
            Pick a display name before you start. Avatar and banner are optional
            and can be added now or later.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <Group label={t('usernameLabel')}>
            <Input placeholder={t('usernamePlaceholder')} {...r('name')} />
          </Group>

          <Alert variant="info" className="py-2">
            <Info className="h-4 w-4" />
            <AlertDescription className="text-xs">
              {hasCollision
                ? 'That display name is already in use. Duplicate names are allowed, and Sharkord will show a #id label in mentions and member lists to disambiguate people.'
                : 'Display names do not need to be unique. If more than one person uses the same name, Sharkord will show a #id label in mentions and member lists to disambiguate people.'}
            </AlertDescription>
          </Alert>

          <div className="grid gap-6 lg:grid-cols-2">
            <AvatarManager user={ownPublicUser} />
            <BannerManager user={ownPublicUser} />
          </div>

          <div className="flex justify-end">
            <Button onClick={finishSetup} disabled={!canContinue}>
              Continue
            </Button>
          </div>
        </CardContent>
      </Card>
    </div>
  );
});

export { UserOnboarding };
