import { TextChannel } from '@/components/channel-view/text';
import { ResizableSidebar } from '@/components/resizable-sidebar';
import { closeVoiceChatSidebar } from '@/features/app/actions';
import { useVoiceChatSidebar } from '@/features/app/hooks';
import { LocalStorageKey } from '@/helpers/storage';
import { cn } from '@/lib/utils';
import { memo } from 'react';

const MIN_WIDTH = 360;
const MAX_WIDTH = 600;
const DEFAULT_WIDTH = 384;

const VoiceChatSidebar = memo(() => {
  const { isOpen, channelId } = useVoiceChatSidebar();

  if (!channelId) {
    return null;
  }

  return (
    <>
      <div
        className={cn(
          'fixed inset-0 z-40 flex flex-col bg-background lg:hidden',
          !isOpen && 'hidden'
        )}
      >
        <TextChannel channelId={channelId} onClose={closeVoiceChatSidebar} />
      </div>

      <ResizableSidebar
        storageKey={LocalStorageKey.VOICE_CHAT_SIDEBAR_WIDTH}
        minWidth={MIN_WIDTH}
        maxWidth={MAX_WIDTH}
        defaultWidth={DEFAULT_WIDTH}
        edge="left"
        isOpen={isOpen}
        className="hidden lg:flex"
      >
        <div className="flex h-full w-full flex-col">
          <div className="flex flex-1 flex-col overflow-hidden">
            <TextChannel
              channelId={channelId}
              onClose={closeVoiceChatSidebar}
            />
          </div>
        </div>
      </ResizableSidebar>
    </>
  );
});

export { VoiceChatSidebar };
