import type { ReactNode } from 'react';

const INLINE_MARKDOWN_DELIMITERS = ['**', '~~', '`', '*'] as const;

const findNextDelimiter = (
  text: string,
  startIndex: number
): {
  delimiter: (typeof INLINE_MARKDOWN_DELIMITERS)[number];
  index: number;
} | null => {
  for (let index = startIndex; index < text.length; index += 1) {
    if (text[index - 1] === '\\') {
      continue;
    }

    for (const delimiter of INLINE_MARKDOWN_DELIMITERS) {
      if (text.startsWith(delimiter, index)) {
        return { delimiter, index };
      }
    }
  }

  return null;
};

const renderInlineMarkdown = (
  text: string,
  keyPrefix: string
): ReactNode[] | null => {
  const nodes: ReactNode[] = [];
  let cursor = 0;
  let changed = false;

  while (cursor < text.length) {
    const nextDelimiter = findNextDelimiter(text, cursor);

    if (!nextDelimiter) {
      nodes.push(text.slice(cursor));
      break;
    }

    if (nextDelimiter.index > cursor) {
      nodes.push(text.slice(cursor, nextDelimiter.index));
    }

    const { delimiter, index } = nextDelimiter;
    const contentStart = index + delimiter.length;
    const closeIndex = text.indexOf(delimiter, contentStart);

    if (
      closeIndex === -1 ||
      closeIndex === contentStart ||
      text[closeIndex - 1] === '\\'
    ) {
      nodes.push(text.slice(index, contentStart));
      cursor = contentStart;
      continue;
    }

    const innerText = text.slice(contentStart, closeIndex);
    const childKey = `${keyPrefix}-${index}`;
    const nestedNodes =
      delimiter === '`' ? innerText : renderInlineMarkdown(innerText, childKey);

    changed = true;

    if (delimiter === '**') {
      nodes.push(<strong key={childKey}>{nestedNodes ?? innerText}</strong>);
    } else if (delimiter === '*') {
      nodes.push(<em key={childKey}>{nestedNodes ?? innerText}</em>);
    } else if (delimiter === '~~') {
      nodes.push(<del key={childKey}>{nestedNodes ?? innerText}</del>);
    } else {
      nodes.push(<code key={childKey}>{innerText}</code>);
    }

    cursor = closeIndex + delimiter.length;
  }

  return changed ? nodes : null;
};

export { renderInlineMarkdown };
