import type { AcMode, Device, DeviceKind } from '../lib/api';
import { Icon, type IconName } from './Icon';

export const PROVIDER_NAME: Record<Device['provider'], string> = { ewelink: 'Sonoff', tuya: 'Tuya' };

const KIND_ICON: Record<DeviceKind, IconName> = {
  light: 'bulb',
  strip: 'strip',
  switch: 'switch',
  plug: 'plug',
  fan: 'fan',
  ac: 'ac',
  sensor: 'sensor',
};

export function DeviceIcon({ kind, size = 30 }: { kind: DeviceKind; size?: number }) {
  return <Icon name={KIND_ICON[kind]} size={size} />;
}

const AC_MODE: Record<AcMode, string> = { cool: 'Cool', heat: 'Heat', auto: 'Auto', fan: 'Fan', dry: 'Dry' };

const deg = (t: number) => `${Number.isInteger(t) ? t : t.toFixed(1)}°`;

/** One line under a device's name: "On · 80%", "Off", "24.5° · 58%"… */
export function deviceStatus(d: Device): string {
  if (!d.online) return 'Offline';
  const readings = [d.temperature !== null ? deg(d.temperature) : null, d.humidity !== null ? `${Math.round(d.humidity)}%` : null];
  if (d.on === null) return readings.filter(Boolean).join(' · ') || '—';
  if (!d.on) return 'Off';
  const detail = d.ac
    ? `${deg(d.ac.target)} ${AC_MODE[d.ac.mode]}`
    : d.fan_speed !== null
      ? `Speed ${d.fan_speed}`
      : d.power_w !== null
        ? `${Math.round(d.power_w)} W`
        : d.color_mode === 'color' && d.brightness !== null
          ? `Color · ${d.brightness}%`
          : d.brightness !== null
            ? `${d.brightness}%`
            : null;
  return detail ? `On · ${detail}` : 'On';
}
