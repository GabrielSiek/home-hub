import { useEffect, useState } from 'react';

export interface ChatMessage {
  id: number;
  user: string;
  color: string;
  text: string;
}

const IRC = 'wss://irc-ws.chat.twitch.tv:443';
const MAX_MESSAGES = 40;
const RECONNECT_MS = 3000;

/** For chatters without a color of their own, or with one too dark to read on the panel. */
const COLORS = ['#ff7f50', '#1e90ff', '#9acd32', '#ff69b4', '#daa520', '#00c878', '#b19cd9', '#5fb3c2'];

let nextId = 0;

function colorFor(login: string): string {
  let h = 0;
  for (const c of login) h = (h * 31 + c.charCodeAt(0)) | 0;
  return COLORS[Math.abs(h) % COLORS.length];
}

/** Relative luminance of #rrggbb, 0–1. */
function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** A chat line from Twitch IRC (with tags), or null for anything that isn't one. */
export function parsePrivmsg(line: string): ChatMessage | null {
  let rest = line;
  const tags: Record<string, string> = {};
  if (rest.startsWith('@')) {
    const sp = rest.indexOf(' ');
    for (const kv of rest.slice(1, sp).split(';')) {
      const eq = kv.indexOf('=');
      if (eq > 0) tags[kv.slice(0, eq)] = kv.slice(eq + 1);
    }
    rest = rest.slice(sp + 1);
  }
  const m = /^:([^!\s]+)!\S+ PRIVMSG #\S+ :(.*)$/.exec(rest);
  if (!m) return null;
  const [, login, body] = m;
  const action = /^\u0001ACTION (.*)\u0001$/.exec(body);
  const own = /^#[0-9a-f]{6}$/i.test(tags.color ?? '') ? tags.color : '';
  return {
    id: nextId++,
    user: (tags['display-name'] || login).replace(/\\s/g, ' '),
    color: own && luminance(own) > 0.12 ? own : colorFor(login),
    text: action ? action[1] : body,
  };
}

/**
 * The latest chat messages of `channel`, read anonymously (Twitch's "justinfan" guest
 * login), so no scope is needed to read. Reconnects when the connection drops.
 */
export function useTwitchChat(channel: string): ChatMessage[] {
  const [messages, setMessages] = useState<ChatMessage[]>([]);

  useEffect(() => {
    setMessages([]);
    let ws: WebSocket | null = null;
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;

    const connect = () => {
      const sock = new WebSocket(IRC);
      ws = sock;
      sock.onopen = () => {
        sock.send('CAP REQ :twitch.tv/tags');
        sock.send('PASS SCHMOOPIIE');
        sock.send(`NICK justinfan${10000 + Math.floor(Math.random() * 80000)}`);
        sock.send(`JOIN #${channel.toLowerCase()}`);
      };
      sock.onmessage = (e) => {
        const fresh: ChatMessage[] = [];
        for (const line of String(e.data).split('\r\n')) {
          if (line.startsWith('PING')) sock.send(line.replace('PING', 'PONG'));
          else {
            const msg = line && parsePrivmsg(line);
            if (msg) fresh.push(msg);
          }
        }
        if (fresh.length) setMessages((prev) => [...prev, ...fresh].slice(-MAX_MESSAGES));
      };
      sock.onclose = () => {
        if (!closed) retry = setTimeout(connect, RECONNECT_MS);
      };
    };

    connect();
    return () => {
      closed = true;
      clearTimeout(retry);
      ws?.close();
    };
  }, [channel]);

  return messages;
}
