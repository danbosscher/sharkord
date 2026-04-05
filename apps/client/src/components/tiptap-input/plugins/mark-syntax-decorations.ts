import { Extension } from '@tiptap/core';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import { Decoration, DecorationSet } from '@tiptap/pm/view';

const markSyntaxDecorationsKey = new PluginKey('markSyntaxDecorations');

type TDelimiter = '**' | '*' | '~~' | '`';

const delimiters: TDelimiter[] = ['**', '~~', '`', '*'];

type TToken = {
  delimiter: TDelimiter;
  start: number;
  end: number;
  contentStart: number;
  contentEnd: number;
};

const findNextDelimiter = (
  text: string,
  startIndex: number
): { delimiter: TDelimiter; index: number } | null => {
  for (let index = startIndex; index < text.length; index += 1) {
    if (text[index - 1] === '\\') {
      continue;
    }

    for (const delimiter of delimiters) {
      if (text.startsWith(delimiter, index)) {
        return { delimiter, index };
      }
    }
  }

  return null;
};

const tokenizeInlineMarkdown = (text: string): TToken[] => {
  const tokens: TToken[] = [];
  let cursor = 0;

  while (cursor < text.length) {
    const nextDelimiter = findNextDelimiter(text, cursor);

    if (!nextDelimiter) {
      break;
    }

    const { delimiter, index } = nextDelimiter;
    const contentStart = index + delimiter.length;
    const closeIndex = text.indexOf(delimiter, contentStart);

    if (
      closeIndex === -1 ||
      closeIndex === contentStart ||
      text[closeIndex - 1] === '\\'
    ) {
      cursor = contentStart;
      continue;
    }

    tokens.push({
      delimiter,
      start: index,
      end: closeIndex + delimiter.length,
      contentStart,
      contentEnd: closeIndex
    });

    cursor = closeIndex + delimiter.length;
  }

  return tokens;
};

const getContentClassName = (delimiter: TDelimiter) => {
  switch (delimiter) {
    case '**':
      return 'md-strong';
    case '*':
      return 'md-emphasis';
    case '~~':
      return 'md-strike';
    case '`':
      return 'md-code';
  }
};

export const MarkSyntaxDecorations = Extension.create({
  name: 'markSyntaxDecorations',

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: markSyntaxDecorationsKey,
        props: {
          decorations: (state) => {
            const decorations: Decoration[] = [];

            state.doc.descendants((node, pos) => {
              if (!node.isText || !node.text) {
                return;
              }

              const tokens = tokenizeInlineMarkdown(node.text);

              tokens.forEach((token) => {
                const markerClass = 'md-syntax-marker';
                const contentClass = getContentClassName(token.delimiter);

                decorations.push(
                  Decoration.inline(
                    pos + token.start,
                    pos + token.contentStart,
                    {
                      class: markerClass
                    }
                  ),
                  Decoration.inline(
                    pos + token.contentStart,
                    pos + token.contentEnd,
                    {
                      class: contentClass
                    }
                  ),
                  Decoration.inline(pos + token.contentEnd, pos + token.end, {
                    class: markerClass
                  })
                );
              });
            });

            return DecorationSet.create(state.doc, decorations);
          }
        }
      })
    ];
  }
});
