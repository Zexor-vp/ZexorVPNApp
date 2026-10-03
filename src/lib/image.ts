// Подготовка фото для обращения в поддержку: уменьшаем и сжимаем в JPEG, чтобы не гнать мегабайты через IPC.

const MAX_SIDE = 1600;

export interface PreparedPhoto {
  mime: 'image/jpeg';
  base64: string;
  previewUrl: string;
}

export async function preparePhoto(file: File): Promise<PreparedPhoto> {
  if (!file.type.startsWith('image/')) throw new Error('можно прикладывать только изображения');
  const bitmap = await createImageBitmap(file);
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  const context = canvas.getContext('2d');
  if (!context) throw new Error('не удалось обработать изображение');
  // Прозрачные PNG после JPEG-сжатия стали бы чёрными — подкладываем белый фон.
  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const dataUrl = canvas.toDataURL('image/jpeg', 0.85);
  return { mime: 'image/jpeg', base64: dataUrl.slice(dataUrl.indexOf(',') + 1), previewUrl: dataUrl };
}
