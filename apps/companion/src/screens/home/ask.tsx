// Home's small questions: a name to type (a room, a task, a routine, a device) and "are you
// sure?" before something goes. A sheet like the rest, not the browser's prompt, and one at a time.
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { create } from 'zustand';

import type { Layout } from '../../nav';
import { Sheet, SheetHead } from '../../ui';

interface Question {
  title: string;
  sub?: string;
  /** Asks for text when set (the field's starting value), else only to confirm. */
  value?: string;
  placeholder?: string;
  ok: string;
  danger?: boolean;
  /** More to fill in under the field (the new routine's time). */
  extra?: (set: (v: unknown) => void) => ReactNode;
  done: (answer: { text: string; extra: unknown } | null) => void;
}

const useAsk = create<{ q: Question | null }>(() => ({ q: null }));

function ask(q: Omit<Question, 'done'>) {
  return new Promise<{ text: string; extra: unknown } | null>((done) => {
    useAsk.getState().q?.done(null);
    useAsk.setState({ q: { ...q, done } });
  });
}

/** A name or a line of text; null when cancelled or left empty. */
export async function askText(title: string, opts: { sub?: string; value?: string; placeholder?: string; ok?: string } = {}) {
  const a = await ask({ title, value: opts.value ?? '', placeholder: opts.placeholder, sub: opts.sub, ok: opts.ok ?? 'Save' });
  const text = a?.text.trim();
  return text ? text : null;
}

/** Text with more under it; `extra` gets what the extra controls set. */
export async function askMore<T>(title: string, opts: { placeholder?: string; ok?: string; extra: (set: (v: T) => void) => ReactNode }) {
  const a = await ask({ title, value: '', placeholder: opts.placeholder, ok: opts.ok ?? 'Save', extra: opts.extra as Question['extra'] });
  const text = a?.text.trim();
  return text ? { text, extra: a!.extra as T | undefined } : null;
}

export async function askSure(title: string, sub: string, ok = 'Delete') {
  return (await ask({ title, sub, ok, danger: true })) !== null;
}

export function AskSheet({ layout }: { layout: Layout }) {
  const q = useAsk((s) => s.q);
  const [text, setText] = useState('');
  const extra = useRef<unknown>(undefined);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    setText(q?.value ?? '');
    extra.current = undefined;
    // iOS opens the keyboard only from a tap; elsewhere the field is ready to type in.
    if (q?.value !== undefined) window.setTimeout(() => field.current?.focus(), 60);
  }, [q]);
  if (!q) return null;
  const finish = (yes: boolean) => {
    useAsk.setState({ q: null });
    q.done(yes ? { text, extra: extra.current } : null);
  };
  const asking = q.value !== undefined;
  return (
    <Sheet open onClose={() => finish(false)} label={q.title} layout={layout} width="min(520px, calc(100% - 140px))">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!asking || text.trim()) finish(true);
        }}
        style={{ display: 'flex', flexDirection: 'column' }}
      >
        <SheetHead title={q.title} sub={q.sub} onClose={() => finish(false)} />
        {asking && (
          <input
            ref={field}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={q.placeholder}
            aria-label={q.title}
            maxLength={60}
            enterKeyHint="done"
            style={{
              marginTop: 18,
              height: 52,
              padding: '0 18px',
              boxSizing: 'border-box',
              borderRadius: 16,
              border: '1px solid rgba(255, 255, 255, 0.14)',
              background: 'rgba(10, 10, 16, 0.6)',
              color: '#f2f2f3',
              fontSize: 17,
              outline: 'none',
            }}
          />
        )}
        {q.extra?.((v) => (extra.current = v))}
        <span style={{ marginTop: 20, display: 'flex', gap: 10 }}>
          <button type="button" className="tap" onClick={() => finish(false)} style={{ ...button, flex: '1 1 0', background: 'rgba(255, 255, 255, 0.08)' }}>
            Cancel
          </button>
          <button
            type="submit"
            className="tap"
            disabled={asking && !text.trim()}
            style={{ ...button, flex: '1 1 0', background: q.danger ? '#c93a3f' : '#0149B2', opacity: asking && !text.trim() ? 0.5 : 1 }}
          >
            {q.ok}
          </button>
        </span>
      </form>
    </Sheet>
  );
}

const button = {
  height: 52,
  border: 0,
  borderRadius: 999,
  color: '#ffffff',
  fontSize: 15,
  fontWeight: 600,
  cursor: 'pointer',
} as const;
