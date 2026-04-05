import { UploadHeaders, type TTempFile } from '@sharkord/shared';
import { toast } from 'sonner';
import { getUrlFromServer } from './get-file-url';
import { getSessionStorageItem, SessionStorageKey } from './storage';

const getSafeFileName = (name: string) => {
  return (
    name
      .trim()
      .normalize('NFKD') // decomposes accented chars
      // eslint-disable-next-line no-control-regex
      .replace(/[^\x00-\x7F]/g, '_') // replaces non-ASCII chars with underscore
  );
};

type TUploadFilesProgress = {
  uploadedBytes: number;
  totalBytes: number;
  currentFileIndex: number;
  fileCount: number;
  currentFileName: string;
  currentFileSize: number;
  currentFileUploadedBytes: number;
};

const parseUploadError = (responseText: string, fallbackMessage: string) => {
  try {
    const parsed = JSON.parse(responseText) as { error?: string };

    return parsed.error || fallbackMessage;
  } catch {
    return responseText || fallbackMessage;
  }
};

const uploadFile = async (
  file: File,
  onProgress?: (loaded: number, total: number) => void
) => {
  const url = getUrlFromServer();

  return await new Promise<TTempFile | undefined>((resolve) => {
    const xhr = new XMLHttpRequest();

    xhr.open('POST', `${url}/upload`);
    xhr.responseType = 'json';
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');
    xhr.setRequestHeader(UploadHeaders.TYPE, file.type);
    xhr.setRequestHeader(UploadHeaders.CONTENT_LENGTH, file.size.toString());
    xhr.setRequestHeader(UploadHeaders.ORIGINAL_NAME, getSafeFileName(file.name));
    xhr.setRequestHeader(
      UploadHeaders.TOKEN,
      getSessionStorageItem(SessionStorageKey.TOKEN) ?? ''
    );

    xhr.upload.onprogress = (event) => {
      if (!onProgress) return;

      onProgress(event.loaded, event.lengthComputable ? event.total : file.size);
    };

    xhr.onload = () => {
      if (xhr.status < 200 || xhr.status >= 300) {
        const fallbackMessage = xhr.statusText || 'Failed to upload file.';
        const message =
          typeof xhr.response === 'object' && xhr.response
            ? (xhr.response as { error?: string }).error || fallbackMessage
            : parseUploadError(xhr.responseText, fallbackMessage);

        toast.error(message);
        resolve(undefined);
        return;
      }

      try {
        const tempFile =
          typeof xhr.response === 'object' && xhr.response
            ? (xhr.response as TTempFile)
            : (JSON.parse(xhr.responseText) as TTempFile);

        resolve(tempFile);
      } catch {
        toast.error('Failed to read the uploaded file response.');
        resolve(undefined);
      }
    };

    xhr.onerror = () => {
      toast.error('File upload failed.');
      resolve(undefined);
    };

    xhr.onabort = () => {
      resolve(undefined);
    };

    xhr.send(file);
  });
};

const uploadFiles = async (
  files: File[],
  onProgress?: (progress: TUploadFilesProgress) => void
) => {
  const uploadedFiles: TTempFile[] = [];
  const totalBytes = files.reduce((acc, file) => acc + file.size, 0);
  let uploadedBytes = 0;

  for (const [index, file] of files.entries()) {
    onProgress?.({
      uploadedBytes,
      totalBytes,
      currentFileIndex: index + 1,
      fileCount: files.length,
      currentFileName: file.name,
      currentFileSize: file.size,
      currentFileUploadedBytes: 0
    });

    const uploadedFile = await uploadFile(file, (loaded, total) => {
      onProgress?.({
        uploadedBytes: uploadedBytes + loaded,
        totalBytes,
        currentFileIndex: index + 1,
        fileCount: files.length,
        currentFileName: file.name,
        currentFileSize: total,
        currentFileUploadedBytes: loaded
      });
    });

    uploadedBytes += file.size;

    onProgress?.({
      uploadedBytes,
      totalBytes,
      currentFileIndex: index + 1,
      fileCount: files.length,
      currentFileName: file.name,
      currentFileSize: file.size,
      currentFileUploadedBytes: file.size
    });

    if (!uploadedFile) continue;

    uploadedFiles.push(uploadedFile);
  }

  return uploadedFiles;
};

export { uploadFile, uploadFiles };
export type { TUploadFilesProgress };
