import { setIsAutoConnecting } from '@/features/app/actions';
import { useIsAppLoading, useIsPluginsLoading } from '@/features/app/hooks';
import { connect } from '@/features/server/actions';
import { useDisconnectInfo, useIsConnected } from '@/features/server/hooks';
import {
  getLocalStorageItem,
  getLocalStorageItemBool,
  getSessionStorageItem,
  LocalStorageKey,
  removeLocalStorageItem,
  SessionStorageKey,
  setLocalStorageItemBool,
  setSessionStorageItem
} from '@/helpers/storage';
import { DisconnectCode } from '@sharkord/shared';
import { memo, useCallback, useEffect, useRef } from 'react';

const AutoLoginController = memo(() => {
  const isConnected = useIsConnected();
  const isAppLoading = useIsAppLoading();
  const isPluginsLoading = useIsPluginsLoading();
  const disconnectInfo = useDisconnectInfo();
  const autoLoginAttempted = useRef(false);
  const transientReconnectAttempted = useRef<string | null>(null);

  const attemptTransientReconnect = useCallback(() => {
    if (isAppLoading || isPluginsLoading || isConnected || !disconnectInfo) {
      return;
    }

    if (
      disconnectInfo.code === DisconnectCode.KICKED ||
      disconnectInfo.code === DisconnectCode.BANNED
    ) {
      return;
    }

    if (!navigator.onLine) {
      return;
    }

    const attemptKey = `${disconnectInfo.code}:${disconnectInfo.reason ?? ''}:${disconnectInfo.time.getTime()}`;

    if (transientReconnectAttempted.current === attemptKey) {
      return;
    }

    const sessionToken = getSessionStorageItem(SessionStorageKey.TOKEN);
    const autoLoginEnabled = getLocalStorageItemBool(
      LocalStorageKey.AUTO_LOGIN
    );
    const savedToken = getLocalStorageItem(LocalStorageKey.AUTO_LOGIN_TOKEN);
    const reconnectToken =
      sessionToken || (autoLoginEnabled ? savedToken : undefined);

    if (!reconnectToken) {
      return;
    }

    transientReconnectAttempted.current = attemptKey;
    setSessionStorageItem(SessionStorageKey.TOKEN, reconnectToken);
    setIsAutoConnecting(true);

    connect()
      .catch(() => {
        // leave the disconnected screen in place if the retry fails
      })
      .finally(() => {
        setIsAutoConnecting(false);
      });
  }, [disconnectInfo, isAppLoading, isPluginsLoading, isConnected]);

  useEffect(() => {
    if (isConnected || !disconnectInfo) {
      transientReconnectAttempted.current = null;
    }
  }, [isConnected, disconnectInfo]);

  useEffect(() => {
    if (
      isAppLoading ||
      isPluginsLoading ||
      isConnected ||
      autoLoginAttempted.current
    ) {
      // ignore if the app is not done loading, if we're already connected or in the process of connecting
      return;
    }

    const autoLoginEnabled = getLocalStorageItemBool(
      LocalStorageKey.AUTO_LOGIN
    );

    const savedToken = getLocalStorageItem(LocalStorageKey.AUTO_LOGIN_TOKEN);

    if (!autoLoginEnabled || !savedToken) {
      // auto-login not enabled or no token saved, do nothing
      return;
    }

    autoLoginAttempted.current = true;

    setIsAutoConnecting(true);
    setSessionStorageItem(SessionStorageKey.TOKEN, savedToken);

    connect()
      .catch(() => {
        // token expired or invalid clear auto-login state so the user
        // sees the connect screen and can log in manually
        removeLocalStorageItem(LocalStorageKey.AUTO_LOGIN_TOKEN);
        setLocalStorageItemBool(LocalStorageKey.AUTO_LOGIN, false);
      })
      .finally(() => {
        // reset auto-login attempt state so if the user logs out and back in they can try auto-login again
        autoLoginAttempted.current = false;
        setIsAutoConnecting(false);
      });
  }, [isAppLoading, isPluginsLoading, isConnected, disconnectInfo]);

  useEffect(() => {
    attemptTransientReconnect();
  }, [attemptTransientReconnect]);

  useEffect(() => {
    const handleOnline = () => {
      transientReconnectAttempted.current = null;
      attemptTransientReconnect();
    };

    const handleVisibilityChange = () => {
      if (document.visibilityState !== 'visible') {
        return;
      }

      transientReconnectAttempted.current = null;
      attemptTransientReconnect();
    };

    window.addEventListener('online', handleOnline);
    document.addEventListener('visibilitychange', handleVisibilityChange);

    return () => {
      window.removeEventListener('online', handleOnline);
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [attemptTransientReconnect]);

  return null;
});

export { AutoLoginController };
