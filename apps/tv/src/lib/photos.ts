import { create } from 'zustand';

import { api, on, type PhotosSnapshot } from './api';
import { toast } from './toast';

/** The screensaver's albums, the Google sign-in and any pick in progress, pushed by the backend. */
export const usePhotos = create<PhotosSnapshot>(() => ({
  albums: [],
  google: { configured: false, signed_in: false, connected: false, error: null },
  pick: null,
}));

const apply = (s: PhotosSnapshot) => usePhotos.setState(s);

/** Keeps `usePhotos` in sync; call once from the root. Returns a cleanup. */
export function syncPhotos(): () => void {
  api.photos().then(apply).catch(() => {});
  return on<PhotosSnapshot>('photos', apply);
}

const plural = (n: number) => (n === 1 ? '1 photo' : `${n} photos`);

export const photos = {
  /** Picks photos on the phone into a new album, or `album` again. Signs in first if needed. */
  pick: (album?: string) =>
    api
      .photosPick(album)
      .then((a) => toast(`${plural(a.count)} saved to ${a.name}`))
      .catch((e) => String(e) !== 'Cancelled' && toast(e)),
  pickHere: () => api.photosPickHere().catch(toast),
  cancel: () => api.photosCancelPick().catch(toast),
  signIn: () =>
    api
      .photosSignIn()
      .then(() => toast('Signed in to Google Photos'))
      .catch(toast),
  signOut: () => api.photosSignOut().catch(toast),
  /** Resolves true once the folder is added. */
  addFolder: (path: string) =>
    api
      .photosAddFolder(path)
      .then((a) => {
        toast(`${a.name}: ${plural(a.count)}`);
        return true;
      })
      .catch((e) => {
        toast(e);
        return false;
      }),
  setEnabled: (id: string, enabled: boolean) => api.photosSetEnabled(id, enabled).catch(toast),
  remove: (id: string) => api.photosRemove(id).catch(toast),
};
