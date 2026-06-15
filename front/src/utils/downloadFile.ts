/** Déclenche le téléchargement d'un fichier statique servi depuis `public/`. */
export function downloadPublicAsset(publicPath: string, filename: string): void {
  const anchor = document.createElement('a');
  anchor.href = publicPath;
  anchor.download = filename;
  anchor.rel = 'noopener';
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
}

/** Charge un asset public en texte (ex. exemple CSV). */
export async function fetchPublicText(publicPath: string): Promise<string> {
  const response = await fetch(publicPath);
  if (!response.ok) {
    throw new Error(`Fichier introuvable : ${publicPath}`);
  }
  return response.text();
}
