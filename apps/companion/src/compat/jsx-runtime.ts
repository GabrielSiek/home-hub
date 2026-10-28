// The JSX runtime the companion builds with (vite.config.ts): React's, except that inline
// styles lose what old Safari can't read. iOS 12's Safari drops `inset` (Safari 14.1), so it
// becomes top/right/bottom/left before React sets it.
import { Fragment, jsx as reactJsx, jsxs as reactJsxs } from 'react/jsx-runtime';

type Props = { style?: Record<string, unknown> } | null | undefined;

export function fixStyle<P extends Props>(props: P): P {
  const style = props?.style;
  if (!style || style.inset === undefined) return props;
  const { inset, ...rest } = style;
  const parts = typeof inset === 'number' ? [inset] : String(inset).trim().split(/\s+/);
  const [top, right = top, bottom = top, left = right] = parts;
  return { ...props, style: { top, right, bottom, left, ...rest } };
}

export { Fragment };

export const jsx: typeof reactJsx = (type, props, key) => reactJsx(type, fixStyle(props as Props), key);
export const jsxs: typeof reactJsxs = (type, props, key) => reactJsxs(type, fixStyle(props as Props), key);
