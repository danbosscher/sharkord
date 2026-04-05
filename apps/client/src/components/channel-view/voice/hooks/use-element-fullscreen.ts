import { useCallback, useEffect, useState, type RefObject } from 'react';

const useElementFullscreen = (elementRef: RefObject<HTMLElement | null>) => {
  const [isFullscreen, setIsFullscreen] = useState(false);

  useEffect(() => {
    const updateFullscreenState = () => {
      setIsFullscreen(document.fullscreenElement === elementRef.current);
    };

    updateFullscreenState();
    document.addEventListener('fullscreenchange', updateFullscreenState);

    return () => {
      document.removeEventListener('fullscreenchange', updateFullscreenState);
    };
  }, [elementRef]);

  const toggleFullscreen = useCallback(async () => {
    const element = elementRef.current;

    if (!element?.requestFullscreen) return;

    if (document.fullscreenElement === element) {
      if (document.exitFullscreen) {
        await document.exitFullscreen();
      }

      return;
    }

    await element.requestFullscreen();
  }, [elementRef]);

  return {
    isFullscreen,
    isFullscreenSupported: document.fullscreenEnabled,
    toggleFullscreen
  };
};

export { useElementFullscreen };
