import { describe, expect, test } from 'bun:test';
import { prepareMarkdownMessageHtml } from '../prepare-markdown-message-html';

describe('prepareMarkdownMessageHtml', () => {
  test('converts inline markdown inside editor html', () => {
    expect(
      prepareMarkdownMessageHtml('<p>hello **world** and *friend*</p>')
    ).toBe('<p>hello <strong>world</strong> and <em>friend</em></p>');
  });

  test('converts strike and code spans', () => {
    expect(prepareMarkdownMessageHtml('<p>~~old~~ `code`</p>')).toBe(
      '<p><del>old</del> <code>code</code></p>'
    );
  });

  test('preserves mention markup while converting surrounding markdown', () => {
    expect(
      prepareMarkdownMessageHtml(
        '<p>ping **now** <span data-type="mention" data-user-id="42" class="mention">@User</span></p>'
      )
    ).toBe(
      '<p>ping <strong>now</strong> <span data-type="mention" data-user-id="42" class="mention">@User</span></p>'
    );
  });

  test('does not transform markdown inside code blocks', () => {
    expect(
      prepareMarkdownMessageHtml(
        '<p><code>**literal**</code> and <pre>~~raw~~</pre></p>'
      )
    ).toBe('<p><code>**literal**</code> and <pre>~~raw~~</pre></p>');
  });

  test('keeps links linkified after markdown conversion', () => {
    expect(
      prepareMarkdownMessageHtml('<p>see **https://example.com**</p>')
    ).toContain(
      '<strong><a href="https://example.com" target="_blank" rel="noopener noreferrer">https://example.com</a></strong>'
    );
  });
});
