// The dev server's JSX runtime, with the same style fix as jsx-runtime.ts.
import { Fragment, jsxDEV as reactJsxDEV } from 'react/jsx-dev-runtime';

import { fixStyle } from './jsx-runtime';

export { Fragment };

export const jsxDEV: typeof reactJsxDEV = (type, props, key, isStatic, source, self) =>
  reactJsxDEV(type, fixStyle(props as { style?: Record<string, unknown> }), key, isStatic, source, self);
