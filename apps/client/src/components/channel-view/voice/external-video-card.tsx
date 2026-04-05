import { cn } from '@/lib/utils';
import { IconButton } from '@sharkord/ui';
import { Video, ZoomIn, ZoomOut } from 'lucide-react';
import { memo, useCallback } from 'react';
import { CardControls } from './card-controls';
import { CardGradient } from './card-gradient';
import { FullscreenButton } from './fullscreen-button';
import { useElementFullscreen } from './hooks/use-element-fullscreen';
import { useScreenShareZoom } from './hooks/use-screen-share-zoom';
import { useVoiceRefs } from './hooks/use-voice-refs';
import { PinButton } from './pin-button';

type TExternalVideoControlsProps = {
  isPinned: boolean;
  isZoomEnabled: boolean;
  isFullscreen: boolean;
  handlePinToggle: () => void;
  handleToggleZoom: () => void;
  handleToggleFullscreen: () => void;
  showPinControls: boolean;
  showFullscreenControl: boolean;
};

const ExternalVideoControls = memo(
  ({
    isPinned,
    isZoomEnabled,
    isFullscreen,
    handlePinToggle,
    handleToggleZoom,
    handleToggleFullscreen,
    showPinControls,
    showFullscreenControl
  }: TExternalVideoControlsProps) => {
    return (
      <CardControls>
        {showFullscreenControl && (
          <FullscreenButton
            isFullscreen={isFullscreen}
            handleToggleFullscreen={handleToggleFullscreen}
          />
        )}
        {showPinControls && isPinned && (
          <IconButton
            variant={isZoomEnabled ? 'default' : 'ghost'}
            icon={isZoomEnabled ? ZoomOut : ZoomIn}
            onClick={handleToggleZoom}
            title={isZoomEnabled ? 'Disable Zoom' : 'Enable Zoom'}
            size="sm"
          />
        )}
        {showPinControls && (
          <PinButton isPinned={isPinned} handlePinToggle={handlePinToggle} />
        )}
      </CardControls>
    );
  }
);

type TExternalVideoCardProps = {
  streamId: number;
  isPinned?: boolean;
  onPin: () => void;
  onUnpin: () => void;
  className?: string;
  showPinControls: boolean;
  name?: string;
};

const ExternalVideoCard = memo(
  ({
    streamId,
    isPinned = false,
    onPin,
    onUnpin,
    className,
    showPinControls = true,
    name
  }: TExternalVideoCardProps) => {
    const { externalVideoRef, hasExternalVideoStream } = useVoiceRefs(streamId);

    const {
      containerRef,
      isZoomEnabled,
      zoom,
      position,
      isDragging,
      handleToggleZoom,
      handleWheel,
      handleMouseDown,
      handleMouseMove,
      handleMouseUp,
      getCursor,
      resetZoom
    } = useScreenShareZoom();
    const { isFullscreen, isFullscreenSupported, toggleFullscreen } =
      useElementFullscreen(containerRef);

    const handlePinToggle = useCallback(() => {
      if (isPinned) {
        onUnpin?.();
        resetZoom();
      } else {
        onPin?.();
      }
    }, [isPinned, onPin, onUnpin, resetZoom]);

    const handleToggleFullscreen = useCallback(() => {
      resetZoom();
      void toggleFullscreen();
    }, [resetZoom, toggleFullscreen]);

    if (!hasExternalVideoStream) return null;

    return (
      <div
        ref={containerRef}
        className={cn(
          'relative bg-card overflow-hidden group',
          'flex items-center justify-center',
          'w-full h-full',
          isFullscreen ? 'rounded-none border-0' : 'rounded-lg border border-border',
          className
        )}
        onWheel={handleWheel}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        style={{
          cursor: getCursor()
        }}
      >
        <CardGradient />

        <ExternalVideoControls
          isPinned={isPinned}
          isZoomEnabled={isZoomEnabled}
          isFullscreen={isFullscreen}
          handlePinToggle={handlePinToggle}
          handleToggleZoom={handleToggleZoom}
          handleToggleFullscreen={handleToggleFullscreen}
          showPinControls={showPinControls}
          showFullscreenControl={isFullscreenSupported}
        />

        <video
          ref={externalVideoRef}
          autoPlay
          muted
          playsInline
          className="absolute inset-0 w-full h-full object-contain bg-black"
          style={{
            transform: `scale(${zoom}) translate(${position.x / zoom}px, ${position.y / zoom}px)`,
            transition: isDragging ? 'none' : 'transform 0.1s ease-out'
          }}
        />

        <div className="absolute bottom-0 left-0 right-0 p-2 z-10 opacity-100 transition-opacity md:opacity-0 md:group-hover:opacity-100">
          <div className="flex items-center gap-2 min-w-0">
            <Video className="size-3.5 text-blue-400 flex-shrink-0" />
            <span className="text-white font-medium text-xs truncate">
              {name || 'External Video'}
            </span>
            {isZoomEnabled && zoom > 1 && (
              <span className="text-white/70 text-xs ml-auto flex-shrink-0">
                {Math.round(zoom * 100)}%
              </span>
            )}
          </div>
        </div>
      </div>
    );
  }
);

ExternalVideoCard.displayName = 'ExternalVideoCard';

export { ExternalVideoCard };
