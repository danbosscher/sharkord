import { prepareMessageHtml } from '@sharkord/shared';

const INLINE_MARKDOWN_DELIMITERS = ['**', '~~', '`', '*'] as const;
const VOID_TAGS = new Set(['br', 'img', 'hr']);
const SKIP_MARKDOWN_TAGS = new Set([
  'a',
  'code',
  'pre',
  'script',
  'style',
  'textarea'
]);

const escapeHtml = (value: string): string =>
  value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');

const getTagName = (token: string): string | null => {
  const match = token.match(/^<\/?\s*([a-z0-9-]+)/i);

  return match?.[1]?.toLowerCase() ?? null;
};

const isClosingTag = (token: string) => /^<\//.test(token);

const isSelfClosingTag = (token: string, name: string | null): boolean => {
  if (!name) return false;

  return VOID_TAGS.has(name) || /\/\s*>$/.test(token);
};

const shouldSkipMarkdown = (token: string, name: string | null): boolean => {
  if (!name) return false;

  if (SKIP_MARKDOWN_TAGS.has(name)) return true;

  return (
    name === 'span' && /data-type\s*=\s*['"](mention|emoji)['"]/i.test(token)
  );
};

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

const renderInlineMarkdownHtml = (text: string): string => {
  let result = '';
  let cursor = 0;

  while (cursor < text.length) {
    const nextDelimiter = findNextDelimiter(text, cursor);

    if (!nextDelimiter) {
      result += text.slice(cursor);
      break;
    }

    if (nextDelimiter.index > cursor) {
      result += text.slice(cursor, nextDelimiter.index);
    }

    const { delimiter, index } = nextDelimiter;
    const contentStart = index + delimiter.length;
    const closeIndex = text.indexOf(delimiter, contentStart);

    if (
      closeIndex === -1 ||
      closeIndex === contentStart ||
      text[closeIndex - 1] === '\\'
    ) {
      result += text.slice(index, contentStart);
      cursor = contentStart;
      continue;
    }

    const innerText = text.slice(contentStart, closeIndex);
    const nested =
      delimiter === '`' ? innerText : renderInlineMarkdownHtml(innerText);

    if (delimiter === '**') {
      result += `<strong>${nested}</strong>`;
    } else if (delimiter === '*') {
      result += `<em>${nested}</em>`;
    } else if (delimiter === '~~') {
      result += `<del>${nested}</del>`;
    } else {
      result += `<code>${innerText}</code>`;
    }

    cursor = closeIndex + delimiter.length;
  }

  return result;
};

const transformMarkdownInHtml = (html: string): string => {
  if (!html) {
    return html;
  }

  const hasHtmlTags = /<[^>]+>/.test(html);

  if (!hasHtmlTags) {
    return renderInlineMarkdownHtml(
      escapeHtml(html).replaceAll('\n', '<br />')
    );
  }

  const tokens = html.split(/(<[^>]+>)/g);
  const stack: { name: string; skipMarkdown: boolean }[] = [];

  return tokens
    .map((token) => {
      if (!token) {
        return token;
      }

      if (!token.startsWith('<')) {
        const shouldSkip = stack.some((entry) => entry.skipMarkdown);

        if (shouldSkip) {
          return token;
        }

        return renderInlineMarkdownHtml(token);
      }

      const name = getTagName(token);

      if (!name) {
        return token;
      }

      if (isClosingTag(token)) {
        for (let index = stack.length - 1; index >= 0; index -= 1) {
          if (stack[index]?.name === name) {
            stack.splice(index, 1);
            break;
          }
        }

        return token;
      }

      if (!isSelfClosingTag(token, name)) {
        stack.push({
          name,
          skipMarkdown: shouldSkipMarkdown(token, name)
        });
      }

      return token;
    })
    .join('');
};

const prepareMarkdownMessageHtml = (html: string): string =>
  prepareMessageHtml(transformMarkdownInHtml(html));

export {
  prepareMarkdownMessageHtml,
  renderInlineMarkdownHtml,
  transformMarkdownInHtml
};
