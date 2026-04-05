import { PluginSlotRenderer } from '@/components/plugin-slot-renderer';
import {
  TiptapInput,
  type TTiptapInputHandle
} from '@/components/tiptap-input';
import { useChannelById } from '@/features/server/channels/hooks';
import {
  useCan,
  useChannelCan,
  usePublicServerSettings
} from '@/features/server/hooks';
import { useFlatPluginCommands } from '@/features/server/plugins/hooks';
import { useOwnUserId, useUserById } from '@/features/server/users/hooks';
import { getRenderedUsername } from '@/helpers/get-rendered-username';
import { useUploadFiles } from '@/hooks/use-upload-files';
import { getTRPCClient } from '@/lib/trpc';
import type { TReplyTarget } from '@/types';
import type { TJoinedPublicUser, TTempFile } from '@sharkord/shared';
import {
  ChannelPermission,
  ChannelType,
  Permission,
  PluginSlot,
  isEmptyMessage
} from '@sharkord/shared';
import { Button, Spinner } from '@sharkord/ui';
import { filesize } from 'filesize';
import { Paperclip, Reply, Send, X } from 'lucide-react';
import {
  memo,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type Ref
} from 'react';
import { useTranslation } from 'react-i18next';
import { FileCard } from '../channel-view/text/file-card';
import { useMessageAuthorName } from '../channel-view/text/hooks/use-message-author-name';
import { UsersTypingIndicator } from '../channel-view/text/users-typing';

type TMessageComposeProps = {
  channelId: number;
  message: string;
  onMessageChange: (value: string) => void;
  onSend: (message: string, files: TTempFile[]) => Promise<boolean>;
  onTyping: () => void;
  typingUsers: TJoinedPublicUser[];
  showPluginSlot?: boolean;
  replyTarget?: TReplyTarget;
  onCancelReply?: () => void;
  ref?: Ref<TMessageComposeHandle>;
};

type TMessageComposeHandle = {
  clearFiles: () => void;
};

const MessageCompose = memo(
  ({
    channelId,
    message,
    onMessageChange,
    onSend,
    onTyping,
    typingUsers,
    showPluginSlot = false,
    replyTarget,
    onCancelReply,
    ref
  }: TMessageComposeProps) => {
    const { t } = useTranslation('common');
    const sendingRef = useRef(false);
    const containerRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<TTiptapInputHandle>(null);
    const [sending, setSending] = useState(false);
    const can = useCan();
    const channelCan = useChannelCan(channelId);
    const channel = useChannelById(channelId);
    const ownUserId = useOwnUserId();
    const publicSettings = usePublicServerSettings();
    const allPluginCommands = useFlatPluginCommands();
    const replyAuthorName = useMessageAuthorName({
      userId: replyTarget?.userId ?? 0,
      pluginId: replyTarget?.pluginId ?? ''
    });
    const dmRecipientId = useMemo(() => {
      if (!channel?.isDm || !channel.name.startsWith('DM - ')) {
        return null;
      }

      const match = channel.name.match(/^DM - (\d+):(\d+)$/);
      if (!match) return null;

      const participants = [Number(match[1]), Number(match[2])];

      return participants.find((id) => id !== ownUserId) ?? null;
    }, [channel?.isDm, channel?.name, ownUserId]);
    const dmRecipient = useUserById(dmRecipientId);

    const placeholder = useMemo(() => {
      if (!channel) {
        return 'Message';
      }

      if (channel.isDm) {
        const recipientName = dmRecipient
          ? getRenderedUsername(dmRecipient)
          : 'Direct Message';

        return `Message @${recipientName}`;
      }

      if (channel.type === ChannelType.VOICE) {
        return `Message ${channel.name}`;
      }

      return `Message #${channel.name}`;
    }, [channel, dmRecipient]);

    const canSendMessages = useMemo(() => {
      return (
        can(Permission.SEND_MESSAGES) &&
        channelCan(ChannelPermission.SEND_MESSAGES)
      );
    }, [can, channelCan]);

    const canUploadFiles = useMemo(() => {
      const canShareFilesInDm =
        !channel?.isDm || !!publicSettings?.storageFileSharingInDirectMessages;

      return (
        can(Permission.SEND_MESSAGES) &&
        can(Permission.UPLOAD_FILES) &&
        channelCan(ChannelPermission.SEND_MESSAGES) &&
        canShareFilesInDm
      );
    }, [can, channelCan, channel, publicSettings]);

    const pluginCommands = useMemo(
      () => (can(Permission.USE_PLUGINS) ? allPluginCommands : undefined),
      [can, allPluginCommands]
    );

    const {
      files,
      removeFile,
      clearFiles,
      uploading,
      uploadingSize,
      uploadProgress,
      openFileDialog,
      fileInputProps
    } = useUploadFiles(channelId, containerRef, !canSendMessages);

    useImperativeHandle(ref, () => ({ clearFiles }), [clearFiles]);

    const handleSend = useCallback(async () => {
      if (
        (isEmptyMessage(message) && !files.length) ||
        !canSendMessages ||
        sendingRef.current
      ) {
        return;
      }

      setSending(true);
      sendingRef.current = true;

      const maxFilesPerMessage =
        publicSettings?.storageMaxFilesPerMessage ?? Number.MAX_SAFE_INTEGER;
      const filesToSend = files.slice(0, Math.max(0, maxFilesPerMessage));

      const success = await onSend(message, filesToSend);

      sendingRef.current = false;
      setSending(false);

      if (success) {
        clearFiles();
      }
    }, [message, files, canSendMessages, onSend, clearFiles, publicSettings]);

    const onRemoveFileClick = useCallback(
      async (fileId: string) => {
        removeFile(fileId);

        const trpc = getTRPCClient();

        try {
          trpc.files.deleteTemporary.mutate({ fileId });
        } catch {
          // ignore error
        }
      },
      [removeFile]
    );

    const uploadPercent = useMemo(() => {
      if (!uploadProgress?.totalBytes) return 0;

      return Math.min(
        100,
        Math.round(
          (uploadProgress.uploadedBytes / uploadProgress.totalBytes) * 100
        )
      );
    }, [uploadProgress]);

    useEffect(() => {
      // focus the input when user clicks on reply
      if (replyTarget) {
        inputRef.current?.focus();
      }
    }, [replyTarget]);

    return (
      <div
        ref={containerRef}
        className="flex shrink-0 flex-col gap-2 p-2 pb-[calc(env(safe-area-inset-bottom)+0.5rem)]"
      >
        {uploading && (
          <div className="rounded-lg border border-border/60 bg-secondary/30 px-3 py-2">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="text-xs font-medium text-foreground">
                  Uploading files{' '}
                  {uploadProgress?.fileCount
                    ? `(${uploadProgress.currentFileIndex}/${uploadProgress.fileCount})`
                    : ''}
                </div>
                <div className="truncate text-xs text-muted-foreground">
                  {uploadProgress?.currentFileName || 'Preparing upload'}
                  {' · '}
                  {uploadPercent > 0 || uploadProgress?.totalBytes
                    ? `${uploadPercent}% (${filesize(uploadProgress?.uploadedBytes ?? uploadingSize)} / ${filesize(uploadProgress?.totalBytes ?? uploadingSize)})`
                    : filesize(uploadingSize)}
                </div>
              </div>
              <Spinner size="xxs" />
            </div>
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-primary transition-[width] duration-200"
                style={{ width: `${uploadPercent}%` }}
              />
            </div>
          </div>
        )}
        {files.length > 0 && (
          <div className="flex gap-1 flex-wrap">
            {files.map((file) => (
              <FileCard
                key={file.id}
                name={file.originalName}
                extension={file.extension}
                size={file.size}
                onRemove={() => onRemoveFileClick(file.id)}
              />
            ))}
          </div>
        )}

        <UsersTypingIndicator typingUsers={typingUsers} />
        <div className="flex items-center gap-2 rounded-lg">
          <div className="flex flex-col gap-1 w-full justify-center">
            {replyTarget && (
              <div className="flex items-center justify-between rounded-md border border-border/60 bg-secondary/40 px-2 py-1 text-xs">
                <div className="min-w-0 flex items-center gap-1.5 text-muted-foreground">
                  <Reply className="h-3.5 w-3.5 shrink-0" />
                  <span>{t('replyingTo', { username: replyAuthorName })}</span>
                </div>
                <Button
                  size="icon"
                  variant="ghost"
                  className="h-6 w-6 shrink-0"
                  onClick={onCancelReply}
                  title={t('cancelReply')}
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              </div>
            )}
            <div className="flex w-full gap-1 items-end">
              <TiptapInput
                ref={inputRef}
                value={message}
                placeholder={placeholder}
                onChange={onMessageChange}
                onSubmit={handleSend}
                onTyping={onTyping}
                disabled={uploading || !canSendMessages}
                readOnly={sending}
                commands={pluginCommands}
              />
              {showPluginSlot && (
                <PluginSlotRenderer slotId={PluginSlot.CHAT_ACTIONS} />
              )}
              <input {...fileInputProps} />
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                disabled={uploading || !canUploadFiles}
                onClick={openFileDialog}
              >
                <Paperclip className="h-4 w-4" />
              </Button>
              <Button
                size="icon"
                variant="ghost"
                className="h-8 w-8"
                onClick={handleSend}
                disabled={uploading || sending || !canSendMessages}
              >
                <Send className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>
      </div>
    );
  }
);

export { MessageCompose, type TMessageComposeHandle };
