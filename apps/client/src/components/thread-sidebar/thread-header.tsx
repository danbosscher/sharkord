import { closeThreadSidebar } from '@/features/app/actions';
import { Button } from '@sharkord/ui';
import { MessageSquareText, X } from 'lucide-react';
import { memo } from 'react';
import { useTranslation } from 'react-i18next';

const ThreadHeader = memo(() => {
  const { t } = useTranslation('common');

  return (
    <div className="flex items-center justify-between px-4 h-12 border-b border-border shrink-0">
      <div className="flex items-center gap-2">
        <MessageSquareText className="h-4 w-4 text-muted-foreground" />
        <span className="font-semibold text-sm">{t('thread')}</span>
      </div>
      <Button
        onClick={closeThreadSidebar}
        variant="ghost"
        size="sm"
        className="h-9 gap-2 rounded-md px-3 hover:bg-accent transition-colors"
      >
        <X className="h-4 w-4" />
        <span className="text-sm">{t('close')}</span>
      </Button>
    </div>
  );
});

export { ThreadHeader };
