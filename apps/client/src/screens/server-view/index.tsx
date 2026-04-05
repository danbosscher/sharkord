import { LeftSidebar } from '@/components/left-sidebar';
import { ModViewSheet } from '@/components/mod-view-sheet';
import { Protect } from '@/components/protect';
import { RightSidebar } from '@/components/right-sidebar';
import { UserOnboarding } from '@/components/server-screens/user-onboarding';
import { ThreadSidebar } from '@/components/thread-sidebar';
import { TopBar } from '@/components/top-bar';
import { VoiceChatSidebar } from '@/components/voice-chat-sidebar';
import { VoiceProvider } from '@/components/voice-provider';
import {
  closeThreadSidebar,
  closeVoiceChatSidebar
} from '@/features/app/actions';
import {
  useSelectedDmChannelId,
  useThreadSidebar,
  useVoiceChatSidebar
} from '@/features/app/hooks';
import { setDmsOpen } from '@/features/server/actions';
import { setSelectedChannelId } from '@/features/server/channels/actions';
import { useDmsOpen, usePublicServerSettings } from '@/features/server/hooks';
import { getLocalStorageItemBool, LocalStorageKey } from '@/helpers/storage';
import { useSwipeGestures } from '@/hooks/use-swipe-gestures';
import { cn } from '@/lib/utils';
import { Permission, TestId } from '@sharkord/shared';
import { memo, useCallback, useEffect, useRef, useState } from 'react';
import { ContentWrapper } from './content-wrapper';
import { PreventBrowser } from './prevent-browser';

const ServerView = memo(() => {
  const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
  const [isMobileUsersOpen, setIsMobileUsersOpen] = useState(false);
  const [isDesktopRightSidebarOpen, setIsDesktopRightSidebarOpen] = useState(
    getLocalStorageItemBool(LocalStorageKey.RIGHT_SIDEBAR_STATE, true)
  );
  const dmsOpen = useDmsOpen();
  const selectedDmChannelId = useSelectedDmChannelId();
  const publicSettings = usePublicServerSettings();
  const previousServerChannelIdRef = useRef<number | undefined>(undefined);
  const { isOpen: isThreadSidebarOpen } = useThreadSidebar();
  const { isOpen: isVoiceChatSidebarOpen } = useVoiceChatSidebar();

  const closeMobileNavigationOverlays = useCallback(() => {
    setIsMobileMenuOpen(false);
    setIsMobileUsersOpen(false);
    closeThreadSidebar();
  }, []);

  const closeAllMobileOverlays = useCallback(() => {
    closeMobileNavigationOverlays();
    closeVoiceChatSidebar();
  }, [closeMobileNavigationOverlays]);

  const handleDesktopRightSidebarToggle = useCallback(() => {
    setIsDesktopRightSidebarOpen((prev) => !prev);
    localStorage.setItem(
      LocalStorageKey.RIGHT_SIDEBAR_STATE,
      !isDesktopRightSidebarOpen ? 'true' : 'false'
    );
  }, [isDesktopRightSidebarOpen]);

  const handleMobileMenuToggle = useCallback(() => {
    closeAllMobileOverlays();
    setIsMobileMenuOpen((prev) => !prev);
  }, [closeAllMobileOverlays]);

  const handleMobileUsersToggle = useCallback(() => {
    closeAllMobileOverlays();
    setIsMobileUsersOpen((prev) => !prev);
  }, [closeAllMobileOverlays]);

  const handleMobileNavigation = useCallback(() => {
    closeAllMobileOverlays();
  }, [closeAllMobileOverlays]);

  const handleSwipeRight = useCallback(() => {
    if (isMobileMenuOpen || isMobileUsersOpen) {
      closeAllMobileOverlays();
      return;
    }

    closeAllMobileOverlays();
    setIsMobileMenuOpen(true);
  }, [closeAllMobileOverlays, isMobileMenuOpen, isMobileUsersOpen]);

  const handleSwipeLeft = useCallback(() => {
    if (isMobileMenuOpen || isMobileUsersOpen) {
      closeAllMobileOverlays();

      return;
    }

    closeAllMobileOverlays();
    setIsMobileUsersOpen(true);
  }, [closeAllMobileOverlays, isMobileMenuOpen, isMobileUsersOpen]);

  const swipeHandlers = useSwipeGestures({
    onSwipeRight: handleSwipeRight,
    onSwipeLeft: handleSwipeLeft
  });

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        closeAllMobileOverlays();
      }
    };

    window.addEventListener('keydown', handleKeyDown);

    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [closeAllMobileOverlays]);

  useEffect(() => {
    const shouldLockScroll =
      isMobileMenuOpen ||
      isMobileUsersOpen ||
      isVoiceChatSidebarOpen ||
      isThreadSidebarOpen;

    if (!shouldLockScroll) {
      return;
    }

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = 'hidden';

    return () => {
      document.body.style.overflow = previousOverflow;
    };
  }, [
    isMobileMenuOpen,
    isMobileUsersOpen,
    isThreadSidebarOpen,
    isVoiceChatSidebarOpen
  ]);

  useEffect(() => {
    if (publicSettings?.directMessagesEnabled === false && dmsOpen) {
      setDmsOpen(false);

      if (previousServerChannelIdRef.current) {
        setSelectedChannelId(previousServerChannelIdRef.current);
      }
    }
  }, [publicSettings?.directMessagesEnabled, dmsOpen]);

  return (
    <VoiceProvider>
      <div
        data-testid={TestId.SERVER_VIEW}
        className="flex h-dvh flex-col bg-background text-foreground dark"
        {...swipeHandlers}
      >
        <TopBar
          onToggleDesktopRightSidebar={handleDesktopRightSidebarToggle}
          isDesktopRightSidebarOpen={isDesktopRightSidebarOpen}
          onToggleMobileLeftSidebar={handleMobileMenuToggle}
          isMobileLeftSidebarOpen={isMobileMenuOpen}
          onToggleMobileRightSidebar={handleMobileUsersToggle}
          isMobileRightSidebarOpen={isMobileUsersOpen}
          onCloseMobileNavigationOverlays={closeMobileNavigationOverlays}
        />
        <div className="relative flex min-h-0 flex-1 overflow-hidden">
          <PreventBrowser />

          {isMobileMenuOpen && (
            <div
              className="md:hidden fixed inset-0 bg-black/50 z-30"
              onClick={closeAllMobileOverlays}
            />
          )}

          {isMobileUsersOpen && (
            <div
              className="lg:hidden fixed inset-0 bg-black/50 z-30"
              onClick={closeAllMobileOverlays}
            />
          )}

          <LeftSidebar
            className={cn(
              'md:relative md:flex fixed inset-0 left-0 h-full z-40 md:z-0 transition-transform duration-300 ease-in-out',
              isMobileMenuOpen
                ? 'translate-x-0'
                : '-translate-x-full md:translate-x-0'
            )}
            onNavigate={handleMobileNavigation}
          />

          <ContentWrapper
            isDmMode={dmsOpen}
            selectedDmChannelId={selectedDmChannelId}
            onOpenChannels={handleMobileMenuToggle}
            onOpenMembers={handleMobileUsersToggle}
          />

          <VoiceChatSidebar />

          <ThreadSidebar isOpen={isThreadSidebarOpen} />

          <RightSidebar
            className={cn(
              'fixed top-0 bottom-0 right-0 h-full z-40',
              'lg:relative lg:z-0',
              isMobileUsersOpen
                ? 'translate-x-0 lg:translate-x-0'
                : 'translate-x-full lg:translate-x-0'
            )}
            isOpen={isMobileUsersOpen || isDesktopRightSidebarOpen}
          />

          <Protect permission={Permission.MANAGE_USERS}>
            <ModViewSheet />
          </Protect>

          <UserOnboarding />
        </div>
      </div>
    </VoiceProvider>
  );
});

export { ServerView };
