import { toggleVoiceChatSidebar } from '@/features/app/actions';
import { useVoiceChatSidebar } from '@/features/app/hooks';
import {
  useCurrentVoiceChannelId,
  useIsCurrentVoiceChannelSelected
} from '@/features/server/channels/hooks';
import {
  usePublicServerSettings,
  useServerName
} from '@/features/server/hooks';
import { cn } from '@/lib/utils';
import { PluginSlot } from '@sharkord/shared';
import { Button, Tooltip } from '@sharkord/ui';
import {
  MessageSquare,
  PanelLeft,
  PanelLeftClose,
  PanelRight,
  PanelRightClose
} from 'lucide-react';
import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { PluginSlotRenderer } from '../plugin-slot-renderer';
import { ServerSearch } from './server-search';
import { VoiceOptionsController } from './voice-options-controller';
import { VolumeController } from './volume-controller';

type TTopBarProps = {
  onToggleDesktopRightSidebar: () => void;
  isDesktopRightSidebarOpen: boolean;
  onToggleMobileLeftSidebar: () => void;
  isMobileLeftSidebarOpen: boolean;
  onToggleMobileRightSidebar: () => void;
  isMobileRightSidebarOpen: boolean;
  onCloseMobileNavigationOverlays?: () => void;
};

const TopBar = memo(
  ({
    onToggleDesktopRightSidebar,
    isDesktopRightSidebarOpen,
    onToggleMobileLeftSidebar,
    isMobileLeftSidebarOpen,
    onToggleMobileRightSidebar,
    isMobileRightSidebarOpen,
    onCloseMobileNavigationOverlays
  }: TTopBarProps) => {
    const { t } = useTranslation('topbar');
    const isCurrentVoiceChannelSelected = useIsCurrentVoiceChannelSelected();
    const currentVoiceChannelId = useCurrentVoiceChannelId();
    const settings = usePublicServerSettings();
    const serverName = useServerName();
    const { isOpen: isAnyVoiceChatOpen, channelId: openVoiceChatChannelId } =
      useVoiceChatSidebar();

    const isVoiceChatOpen =
      isAnyVoiceChatOpen && openVoiceChatChannelId === currentVoiceChannelId;

    const handleToggleVoiceChat = useCallback(() => {
      onCloseMobileNavigationOverlays?.();

      if (currentVoiceChannelId) {
        toggleVoiceChatSidebar(currentVoiceChannelId);
      }
    }, [currentVoiceChannelId, onCloseMobileNavigationOverlays]);

    return (
      <>
        <div className="flex h-12 w-full items-center justify-between border-b border-border bg-card px-2 lg:hidden">
          <Button
            variant="ghost"
            size="sm"
            onClick={onToggleMobileLeftSidebar}
            className="h-8 px-2"
          >
            <Tooltip
              content={
                isMobileLeftSidebarOpen
                  ? t('closeChannelsSidebar')
                  : t('openChannelsSidebar')
              }
            >
              <div>
                {isMobileLeftSidebarOpen ? (
                  <PanelLeftClose className="h-4 w-4" />
                ) : (
                  <PanelLeft className="h-4 w-4" />
                )}
              </div>
            </Tooltip>
          </Button>

          <div className="min-w-0 flex-1 px-3 text-center">
            <span className="block truncate text-sm font-semibold text-foreground">
              {serverName}
            </span>
          </div>

          <div className="flex items-center gap-1">
            {settings?.enableSearch && <ServerSearch compact />}
            {isCurrentVoiceChannelSelected && currentVoiceChannelId && (
              <>
                <VoiceOptionsController />
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleToggleVoiceChat}
                  className="h-8 px-2"
                >
                  <Tooltip
                    content={
                      isVoiceChatOpen ? t('closeVoiceChat') : t('openVoiceChat')
                    }
                    asChild={false}
                  >
                    <MessageSquare
                      className={cn(
                        'h-4 w-4',
                        isVoiceChatOpen && 'fill-current'
                      )}
                    />
                  </Tooltip>
                </Button>
              </>
            )}

            <Button
              variant="ghost"
              size="sm"
              onClick={onToggleMobileRightSidebar}
              className="h-8 px-2"
            >
              <Tooltip
                content={
                  isMobileRightSidebarOpen
                    ? t('closeMembersSidebar')
                    : t('openMembersSidebar')
                }
              >
                <div>
                  {isMobileRightSidebarOpen ? (
                    <PanelRightClose className="h-4 w-4" />
                  ) : (
                    <PanelRight className="h-4 w-4" />
                  )}
                </div>
              </Tooltip>
            </Button>
          </div>
        </div>

        <div className="hidden h-12 w-full grid-cols-[1fr_minmax(320px,1.4fr)_1fr] items-center gap-2 border-b border-border bg-card px-4 transition-all duration-300 ease-in-out lg:grid">
          <div className="flex min-w-0 items-center gap-2" />

          <div className="flex items-center justify-center">
            {settings?.enableSearch && <ServerSearch />}
          </div>

          <div className="flex min-w-0 items-center justify-end gap-2">
            <PluginSlotRenderer slotId={PluginSlot.TOPBAR_RIGHT} />
            {isCurrentVoiceChannelSelected && currentVoiceChannelId && (
              <>
                <VoiceOptionsController />
                <VolumeController channelId={currentVoiceChannelId} />
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleToggleVoiceChat}
                  className="h-7 px-2 transition-all duration-200 ease-in-out"
                >
                  <Tooltip
                    content={
                      isVoiceChatOpen ? t('closeVoiceChat') : t('openVoiceChat')
                    }
                    asChild={false}
                  >
                    <MessageSquare
                      className={cn(
                        'w-4 h-4 transition-all duration-200 ease-in-out',
                        isVoiceChatOpen && 'fill-current'
                      )}
                    />
                  </Tooltip>
                </Button>
              </>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={onToggleDesktopRightSidebar}
              className="h-7 px-2 transition-all duration-200 ease-in-out"
            >
              {isDesktopRightSidebarOpen ? (
                <Tooltip content={t('closeMembersSidebar')}>
                  <div>
                    <PanelRightClose className="w-4 h-4 transition-transform duration-200 ease-in-out" />
                  </div>
                </Tooltip>
              ) : (
                <Tooltip content={t('openMembersSidebar')}>
                  <div>
                    <PanelRight className="w-4 h-4 transition-transform duration-200 ease-in-out" />
                  </div>
                </Tooltip>
              )}
            </Button>
          </div>
        </div>
      </>
    );
  }
);

export { TopBar };
