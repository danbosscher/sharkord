import {
  useVolumeControl,
  type TVolumeKey
} from '@/components/voice-provider/volume-control-context';
import { useOwnUserId, useUserById } from '@/features/server/users/hooks';
import { useVoice } from '@/features/server/voice/hooks';
import { cn } from '@/lib/utils';
import { StreamKind } from '@sharkord/shared';
import { IconButton } from '@sharkord/ui';
import { Eye, EyeOff, Monitor, ZoomIn, ZoomOut } from 'lucide-react';
import { memo, useCallback, useMemo } from 'react';
import { CardControls } from './card-controls';
import { CardGradient } from './card-gradient';
import { FullscreenButton } from './fullscreen-button';
import { useElementFullscreen } from './hooks/use-element-fullscreen';
import { useScreenShareZoom } from './hooks/use-screen-share-zoom';
import { useVideoStats } from './hooks/use-video-stats';
import { useVoiceRefs } from './hooks/use-voice-refs';
import { PinButton } from './pin-button';
import { VolumeButton } from './volume-button';

type tScreenShareControlsProps = {
  isPinned: boolean;
  isZoomEnabled: boolean;
  isFullscreen: boolean;
  handlePinToggle: () => void;
  handleToggleZoom: () => void;
  handleToggleFullscreen: () => void;
  showPinControls: boolean;
  showFullscreenControl: boolean;
  showAudioControl: boolean;
  canToggleVideo: boolean;
  hideVideo: boolean;
  onToggleVideo?: () => void;
  volumeKey: TVolumeKey;
};

const ScreenShareControls = memo(
  ({
    isPinned,
    isZoomEnabled,
    isFullscreen,
    handlePinToggle,
    handleToggleZoom,
    handleToggleFullscreen,
    showPinControls,
    showFullscreenControl,
    showAudioControl,
    canToggleVideo,
    hideVideo,
    onToggleVideo,
    volumeKey
  }: tScreenShareControlsProps) => {
    return (
      <CardControls>
        {canToggleVideo && (
          <IconButton
            variant={hideVideo ? 'default' : 'ghost'}
            icon={hideVideo ? Eye : EyeOff}
            onClick={onToggleVideo}
            title={hideVideo ? 'Show Screen Share' : 'Hide Screen Share'}
            size="sm"
          />
        )}
        {showAudioControl && <VolumeButton volumeKey={volumeKey} />}
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

type TScreenShareCardProps = {
  userId: number;
  isPinned?: boolean;
  onPin: () => void;
  onUnpin: () => void;
  className?: string;
  showPinControls: boolean;
  hideVideo?: boolean;
  canToggleVideo?: boolean;
  onToggleVideo?: () => void;
};

const ScreenShareCard = memo(
  ({
    userId,
    isPinned = false,
    onPin,
    onUnpin,
    className,
    showPinControls = true,
    hideVideo = false,
    canToggleVideo = false,
    onToggleVideo
  }: TScreenShareCardProps) => {
    const user = useUserById(userId);
    const ownUserId = useOwnUserId();
    const { getUserScreenVolumeKey } = useVolumeControl();
    const isOwnUser = ownUserId === userId;
    const volumeKey = getUserScreenVolumeKey(userId);
    const {
      screenShareRef,
      screenShareAudioRef,
      hasScreenShareStream,
      hasScreenShareAudioStream
    } = useVoiceRefs(userId);
    const { transportStats, getConsumerCodec } = useVoice();
    const videoStats = useVideoStats(screenShareRef, hasScreenShareStream);

    const codec = useMemo(() => {
      let mimeType: string | undefined;

      if (isOwnUser) {
        mimeType = transportStats.screenShare?.codec;
      } else {
        mimeType = getConsumerCodec(userId, StreamKind.SCREEN);
      }

      if (!mimeType) return null;

      const parts = mimeType.split('/');

      return parts.length > 1 ? parts[1] : mimeType;
    }, [
      isOwnUser,
      transportStats.screenShare?.codec,
      getConsumerCodec,
      userId
    ]);

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

    if (!user) return null;

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
        onWheel={!hideVideo && hasScreenShareStream ? handleWheel : undefined}
        onMouseDown={
          !hideVideo && hasScreenShareStream ? handleMouseDown : undefined
        }
        onMouseMove={
          !hideVideo && hasScreenShareStream ? handleMouseMove : undefined
        }
        onMouseUp={!hideVideo && hasScreenShareStream ? handleMouseUp : undefined}
        onMouseLeave={
          !hideVideo && hasScreenShareStream ? handleMouseUp : undefined
        }
        style={{
          cursor: !hideVideo && hasScreenShareStream ? getCursor() : 'default'
        }}
      >
        <CardGradient />

        <ScreenShareControls
          isPinned={isPinned}
          isZoomEnabled={isZoomEnabled}
          isFullscreen={isFullscreen}
          handlePinToggle={handlePinToggle}
          handleToggleZoom={handleToggleZoom}
          handleToggleFullscreen={handleToggleFullscreen}
          showPinControls={showPinControls}
          showFullscreenControl={
            isFullscreenSupported && hasScreenShareStream && !hideVideo
          }
          showAudioControl={!isOwnUser && hasScreenShareAudioStream}
          canToggleVideo={canToggleVideo}
          hideVideo={hideVideo}
          onToggleVideo={onToggleVideo}
          volumeKey={volumeKey}
        />

        {hasScreenShareStream && !hideVideo ? (
          <video
            ref={screenShareRef}
            autoPlay
            muted
            playsInline
            className="absolute inset-0 w-full h-full object-contain bg-black"
            style={{
              transform: `scale(${zoom}) translate(${position.x / zoom}px, ${position.y / zoom}px)`,
              transition: isDragging ? 'none' : 'transform 0.1s ease-out'
            }}
          />
        ) : (
          <div className="flex flex-col items-center justify-center gap-4 p-8">
            <div className="w-20 h-20 rounded-full bg-gradient-to-br from-purple-500/30 to-violet-500/30 flex items-center justify-center border-2 border-purple-500/50">
              <Monitor className="size-10 text-purple-300" />
            </div>
          </div>
        )}

        <audio
          ref={screenShareAudioRef}
          className="hidden"
          autoPlay
          playsInline
        />

        <div className="absolute bottom-0 left-0 right-0 p-2 z-10 opacity-100 transition-opacity md:opacity-0 md:group-hover:opacity-100">
          <div className="flex items-center gap-2 min-w-0">
            <Monitor className="size-3.5 text-purple-400 shrink-0" />
            <span className="text-white font-medium text-xs truncate">
              {user.name}'s screen
            </span>
            {!hideVideo && (videoStats || codec) && (
              <span className="text-white/50 text-xs shrink-0">
                {codec}
                {codec && videoStats && ' '}
                {videoStats && (
                  <>
                    {videoStats.width}x{videoStats.height}
                    {videoStats.frameRate > 0 && ` ${videoStats.frameRate}fps`}
                  </>
                )}
              </span>
            )}
            {!hideVideo && isZoomEnabled && zoom > 1 && (
              <span className="text-white/70 text-xs ml-auto shrink-0">
                {Math.round(zoom * 100)}%
              </span>
            )}
          </div>
        </div>
      </div>
    );
  }
);

ScreenShareCard.displayName = 'ScreenShareCard';

export { ScreenShareCard };
