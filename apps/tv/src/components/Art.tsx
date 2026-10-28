import { useState, type ReactNode } from 'react';

import { fileSrc } from '../lib/api';

interface Props {
  /** Candidates, best first: file paths or URLs. The next one is tried when one fails to load. */
  sources: string[];
  className?: string;
  alt?: string;
  /** Shown when no candidate loads. */
  fallback?: ReactNode;
}

export function Art({ sources, className, alt = '', fallback = null }: Props) {
  const key = sources.join('|');
  const [failed, setFailed] = useState({ key, count: 0 });
  const index = failed.key === key ? failed.count : 0;
  if (index >= sources.length) return <>{fallback}</>;
  const src = sources[index];
  return (
    <img
      className={className}
      src={/^https?:/.test(src) ? src : fileSrc(src)}
      alt={alt}
      loading="lazy"
      onError={() => setFailed({ key, count: index + 1 })}
    />
  );
}
