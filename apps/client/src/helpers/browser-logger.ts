import { isDebug } from './is-debug';

const logVoice = (...args: unknown[]) => {
  if (!isDebug()) {
    return;
  }

  console.log(
    '%c[VOICE-PROVIDER]',
    'color: salmon; font-weight: bold;',
    ...args
  );
};

const logDebug = (...args: unknown[]) => {
  if (isDebug()) {
    console.log('%c[DEBUG]', 'color: lightblue; font-weight: bold;', ...args);
  }
};

export { logDebug, logVoice };
