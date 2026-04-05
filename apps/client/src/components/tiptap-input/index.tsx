import { EmojiPicker } from '@/components/emoji-picker';
import { useCustomEmojis } from '@/features/server/emojis/hooks';
import { useFilteredUsers } from '@/features/server/users/hooks';
import type { TCommandInfo } from '@sharkord/shared';
import { Button } from '@sharkord/ui';
import Emoji, { gitHubEmojis } from '@tiptap/extension-emoji';
import Link from '@tiptap/extension-link';
import type { Editor } from '@tiptap/core';
import { EditorContent, useEditor } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import { Smile } from 'lucide-react';
import {
  memo,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
  type Ref
} from 'react';
import type { TEmojiItem } from './helpers';
import {
  COMMANDS_STORAGE_KEY,
  CommandSuggestion
} from './plugins/command-suggestion';
import { MarkSyntaxDecorations } from './plugins/mark-syntax-decorations';
import { Mention } from './plugins/mentions';
import { MentionNode } from './plugins/mentions/node';
import {
  MENTION_STORAGE_KEY,
  MentionSuggestion
} from './plugins/mentions/suggestion';
import { SlashCommands } from './plugins/slash-commands-extension';
import { EmojiSuggestion } from './plugins/suggestions';
import { prepareMarkdownMessageHtml } from '@/helpers/prepare-markdown-message-html';

type TTiptapInputProps = {
  disabled?: boolean;
  readOnly?: boolean;
  value?: string;
  placeholder?: string;
  onChange?: (html: string) => void;
  onSubmit?: () => void;
  onCancel?: () => void;
  onTyping?: () => void;
  commands?: TCommandInfo[];
  ref?: Ref<TTiptapInputHandle>;
};

type TTiptapInputHandle = {
  focus: () => void;
};

const TiptapInput = memo(
  ({
    value,
    placeholder,
    onChange,
    onSubmit,
    onCancel,
    onTyping,
    disabled,
    readOnly,
    commands,
    ref
  }: TTiptapInputProps) => {
    const readOnlyRef = useRef(readOnly);
    const editorRef = useRef<Editor | null>(null);

    readOnlyRef.current = readOnly;

    const [isEmpty, setIsEmpty] = useState(true);

    const customEmojis = useCustomEmojis();
    const users = useFilteredUsers();

    const extensions = useMemo(() => {
      const exts = [
        StarterKit.configure({
          hardBreak: {
            HTMLAttributes: {
              class: 'hard-break'
            }
          }
        }),
        Link.configure({
          autolink: true,
          defaultProtocol: 'https',
          openOnClick: false,
          HTMLAttributes: {
            target: '_blank',
            rel: 'noopener noreferrer'
          },
          shouldAutoLink: (url) => {
            return /^https?:\/\//i.test(url);
          }
        }),
        Emoji.configure({
          emojis: [...customEmojis, ...gitHubEmojis],
          enableEmoticons: true,
          suggestion: EmojiSuggestion,
          HTMLAttributes: {
            class: 'emoji-image'
          }
        }),
        Mention.configure({
          users,
          suggestion: MentionSuggestion
        }),
        MentionNode,
        MarkSyntaxDecorations
      ];

      if (commands) {
        exts.push(
          SlashCommands.configure({
            commands,
            suggestion: CommandSuggestion
            // eslint-disable-next-line @typescript-eslint/no-explicit-any
          }) as any
        );
      }

      return exts;
    }, [customEmojis, commands, users]);

    const editor = useEditor({
      extensions,
      content: value,
      editable: !disabled,
      onCreate: ({ editor }) => {
        editorRef.current = editor;
        setIsEmpty(editor.isEmpty);
      },
      onUpdate: ({ editor }) => {
        const html = editor.getHTML();

        onChange?.(html);
        setIsEmpty(editor.isEmpty);

        if (!editor.isEmpty) {
          onTyping?.();
        }
      },
      editorProps: {
        handleKeyDown: (_view, event) => {
          // block all input when readOnly
          if (readOnlyRef.current) {
            event.preventDefault();
            return true;
          }

          const suggestionElement = document.querySelector('.bg-popover');
          const hasSuggestions =
            suggestionElement && document.body.contains(suggestionElement);

          if (event.key === 'Enter') {
            if (event.shiftKey) {
              return false;
            }

            // if suggestions are active, don't handle Enter - let the suggestion handle it
            if (hasSuggestions) {
              return false;
            }

            event.preventDefault();
            onSubmit?.();
            return true;
          }

          if (event.key === 'Escape') {
            event.preventDefault();
            onCancel?.();
            return true;
          }

          return false;
        },
        handleClickOn: (_view, _pos, _node, _nodePos, event) => {
          const target = event.target as HTMLElement;

          // prevents clicking on links inside the edit from opening them in the browser
          if (target.tagName === 'A') {
            event.preventDefault();

            return true;
          }

          return false;
        },
        handlePaste: (_view, event) => {
          if (readOnlyRef.current) {
            return true;
          }

          const clipboard = event.clipboardData;

          if (!clipboard) {
            return false;
          }

          const text = clipboard.getData('text/plain');
          const html = clipboard.getData('text/html');

          if (!text || html) {
            return false;
          }

          event.preventDefault();

          editorRef.current?.commands.insertContent(
            prepareMarkdownMessageHtml(text)
          );

          return true;
        },
        handleDrop: () => readOnlyRef.current
      }
    });

    useImperativeHandle(
      ref,
      () => ({
        focus: () => editor?.chain().focus().run()
      }),
      [editor]
    );

    const handleEmojiSelect = (emoji: TEmojiItem) => {
      if (disabled || readOnly) return;

      if (emoji.shortcodes.length > 0) {
        editor?.chain().focus().setEmoji(emoji.shortcodes[0]).run();
      }
    };

    // keep emoji storage in sync with custom emojis from the store
    // this ensures newly added emojis appear in autocomplete without refreshing the app
    useEffect(() => {
      if (editor) {
        const allEmojis = [...customEmojis, ...gitHubEmojis];

        if (editor.storage.emoji) {
          editor.storage.emoji.emojis = allEmojis;
        }

        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const applyEmojiOptions = (extension: any) => {
          const typed = extension;

          if (typed.name === 'emoji' && typed.options) {
            typed.options.emojis = allEmojis;
          }
        };

        editor.extensionManager.extensions.forEach(applyEmojiOptions);
        editor.options.extensions?.forEach(applyEmojiOptions);
      }
    }, [editor, customEmojis]);

    // keep commands storage in sync with plugin commands from the store
    useEffect(() => {
      if (editor && commands) {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const storage = editor.storage as any;
        if (storage[COMMANDS_STORAGE_KEY]) {
          storage[COMMANDS_STORAGE_KEY].commands = commands;
        }
      }
    }, [editor, commands]);

    // keep mention users storage in sync with the users from the store
    useEffect(() => {
      if (editor) {
        const storage = editor.storage as unknown as Record<
          string,
          { users?: typeof users }
        >;

        if (storage[MENTION_STORAGE_KEY]) {
          storage[MENTION_STORAGE_KEY].users = users;
        }
      }
    }, [editor, users]);

    useEffect(() => {
      if (editor && value !== undefined) {
        const currentContent = editor.getHTML();

        // only update if content is actually different to avoid cursor jumping
        if (currentContent !== value) {
          editor.commands.setContent(value);
        }
      }
    }, [editor, value]);

    useEffect(() => {
      if (editor) {
        editor.setEditable(!disabled);
      }
    }, [editor, disabled]);

    useEffect(() => {
      if (!editor) return;

      setIsEmpty(editor.isEmpty);
    }, [editor, value]);

    return (
      <div className="flex flex-1 items-end gap-2 min-w-0">
        <div className="relative flex min-w-0 flex-1">
          {placeholder && isEmpty && (
            <div
              aria-hidden="true"
              className="pointer-events-none absolute left-3 top-2.5 z-10 text-sm text-muted-foreground/70 select-none"
            >
              {placeholder}
            </div>
          )}
          <EditorContent
            editor={editor}
            className={`border p-2 rounded w-full min-h-10 max-h-80 tiptap overflow-auto relative transition-colors focus-within:border-ring [&_.ProseMirror:focus]:outline-none ${
              disabled ? 'opacity-50 cursor-not-allowed bg-muted' : ''
            }`}
          />
        </div>

        <EmojiPicker onEmojiSelect={handleEmojiSelect}>
          <Button variant="ghost" size="icon" disabled={disabled}>
            <Smile className="h-5 w-5" />
          </Button>
        </EmojiPicker>
      </div>
    );
  }
);

export { TiptapInput, type TTiptapInputHandle };
