import type { TFile } from '@sharkord/shared';
import { getFileUrl } from './get-file-url';

const downloadFile = async (file: TFile) => {
  const fileUrl = getFileUrl(file);

  if (!fileUrl) {
    console.error('Failed to get file URL.');
    return;
  }

  const response = await fetch(fileUrl);

  if (!response.ok) {
    console.error(`Failed to download file: ${response.statusText}`);
    return;
  }

  const link = document.createElement('a');
  const objectUrl = URL.createObjectURL(await response.blob());

  link.href = objectUrl;
  link.download = file.originalName;

  link.click();

  window.setTimeout(() => {
    URL.revokeObjectURL(objectUrl);
  }, 0);
};

export { downloadFile };
