import { create } from 'zustand';

interface ToastState {
  message: string | null;
}

export const useToast = create<ToastState>(() => ({ message: null }));

let timer: ReturnType<typeof setTimeout> | undefined;

export function toast(message: unknown) {
  clearTimeout(timer);
  useToast.setState({ message: String(message) });
  timer = setTimeout(() => useToast.setState({ message: null }), 5000);
}
