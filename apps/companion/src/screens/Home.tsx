// Home (Tablet · Home · rooms / · bedroom, Mobile · Home · rooms / · living room): the house room
// by room, the same rooms and devices as the TV's Smart Home. The overview has a card per room
// (its All lights, screens, devices, next routine and tasks), what voice heard last and the
// house's own routines; a room has its All lights, its tiles, routines, tasks and screens. Edit
// arranges a room's tiles (order, size, hidden, name, room) and the rooms themselves, saved for
// every screen. It opens on the room this screen is in. Where Home is lives in the address: #/home/room:<name>, a device's panel over it
// …|device:<id>.
import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';

import type { Device, SmartHome } from '@tv/lib/api';
import { houseRooms, isGroup, lightRgb, NO_ROOM, rgba, roomList, switchable, tileSpan, type RoomView } from '@tv/lib/smarthome';
import type { ScreenProps } from '../App';
import { Icon } from '../icons';
import { useLive } from '../live';
import { underSub, useNow, type Layout } from '../nav';
import { thisScreen, thisWord } from '../screen';
import { fadeUp, Round, useBox } from '../ui';
import { addRoom, deleteRoom, renameRoom, setRoomIcon, shiftRoom } from './home/actions';
import { askSure, askText, AskSheet } from './home/ask';
import { goHome, homeAt, PANEL, ROOM } from './home/model';
import { DevicePanel } from './home/panels';
import {
  AddDevicesSheet,
  AllLights,
  EditTile,
  Heard,
  HouseRoutines,
  IconSheet,
  panel,
  RoomCard,
  RoomGlyph,
  roomSummary,
  Routines,
  Screens,
  Tasks,
  TileSheet,
} from './home/parts';
import { GhostTile, PhoneTile, TabletTile } from './home/tiles';

export default function Home({ layout, sub }: ScreenProps) {
  // Under a sheet of the app's own (the assistant), the room it opened over stays on show.
  const own = !sub || sub.startsWith(ROOM) || sub.startsWith(PANEL);
  const { room, panel: open } = homeAt(own ? sub : underSub('home'));
  useOpenHere(sub);
  return (
    <>
      {layout === 'tablet' ? <TabletHome room={room} /> : <PhoneHome room={room} />}
      <DevicePanel layout={layout} id={open} />
      <AskSheet layout={layout} />
    </>
  );
}

/** Opening Home (not at a room or a device already) shows the room this screen is in, once the
 *  launcher's snapshot says which; a screen in none (a phone) gets every room. */
function useOpenHere(sub: string | null) {
  const home = useLive((s) => s.home);
  const decided = useRef(false);
  // Only when it opens: going back to every room afterwards stays there.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `sub` as it was when the snapshot came, not again on every move.
  useEffect(() => {
    if (decided.current || !home) return;
    decided.current = true;
    const here = thisScreen(home)?.room;
    if (!sub && here && roomList(home.devices, home.layout).includes(here)) goHome(here);
  }, [home]);
}

/** The house as rooms, from the launcher's snapshot; the routines' times move on with the clock. */
function useHouse() {
  const home = useLive((s) => s.home);
  const now = useNow(60_000);
  const rooms = useMemo(() => (home ? houseRooms(home, new Date(now)) : []), [home, now]);
  // A device that drops off the network for a moment (a missed poll) keeps its place among the
  // ones that answer, dimmed, so the others don't move up under the finger. One that's been
  // offline since Home opened is only under Offline.
  const seen = useRef(new Set<string>()).current;
  for (const d of home?.devices ?? []) if (d.online) seen.add(d.id);
  const answering = (d: Device) => d.online || seen.has(d.id);
  return { home, rooms, answering };
}

/** A room's two tabs: the devices that answer, and the ones that don't. */
type Shown = 'online' | 'offline';

function configured(home: SmartHome | null) {
  return !!home && (home.local.configured || home.tuya.configured || home.ewelink.configured);
}

/** "4 rooms · 3 of 9 lights on", or why there's nothing to show. */
function houseLine(home: SmartHome | null, rooms: RoomView[]) {
  if (!home) return { text: 'Looking for your devices…', dot: '#8a8a94' };
  if (!configured(home)) return { text: 'No devices set up yet', dot: '#8a8a94' };
  const link = home.local.configured ? home.local : home.tuya.configured ? home.tuya : home.ewelink;
  if (!link.connected) return { text: link.error ?? 'Looking for your devices…', dot: link.error ? '#ffb454' : '#8a8a94' };
  const lights = home.devices.filter((d) => switchable(d) && d.online && !isGroup(d.id) && ['light', 'strip', 'switch'].includes(d.kind));
  const named = rooms.filter((r) => r.id !== NO_ROOM).length;
  return { text: `${named} ${named === 1 ? 'room' : 'rooms'} · ${lights.filter((d) => d.on).length} of ${lights.length} lights on`, dot: '#3FB950' };
}

function Title({ title, line, size }: { title: string; line: { text: string; dot: string }; size: number }) {
  return (
    <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.2 }}>
      <span className="ellipsis" style={{ fontSize: size, fontWeight: 700, letterSpacing: '-0.01em' }}>
        {title}
      </span>
      <span style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12, color: '#a0a0a0', minWidth: 0 }}>
        <span aria-hidden="true" style={{ flex: '0 0 7px', width: 7, height: 7, borderRadius: 999, background: line.dot }} />
        <span className="ellipsis">{line.text}</span>
      </span>
    </span>
  );
}

/** Edit / Done. */
function EditButton({ editing, onClick, phone }: { editing: boolean; onClick: () => void; phone: boolean }) {
  if (phone)
    return (
      <Round
        icon={editing ? 'check' : 'edit'}
        label={editing ? 'Done' : 'Edit'}
        size={42}
        iconSize={18}
        bg={editing ? '#0149B2' : 'rgba(35, 35, 40, 0.8)'}
        border={editing ? '0' : '1px solid #333333'}
        onClick={onClick}
      />
    );
  return (
    <button
      type="button"
      className="tap"
      onClick={onClick}
      style={{
        flex: '0 0 auto',
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        height: 44,
        padding: '0 18px',
        borderRadius: 999,
        border: editing ? 0 : '1px solid #333333',
        background: editing ? '#0149B2' : 'rgba(35, 35, 40, 0.8)',
        color: '#ffffff',
        fontSize: 14,
        fontWeight: 600,
        cursor: 'pointer',
        whiteSpace: 'nowrap',
      }}
    >
      <Icon name={editing ? 'check' : 'edit'} size={17} sw={2} />
      {editing ? 'Done' : 'Edit'}
    </button>
  );
}

const glowAt = (bg: string, blur: number, pos: CSSProperties): CSSProperties => ({
  position: 'absolute',
  borderRadius: 999,
  background: bg,
  filter: `blur(${blur}px)`,
  pointerEvents: 'none',
  ...pos,
});

/** "You're here · this iPad is in the Bedroom", else what's on in the room. */
function where(home: SmartHome | null, r: RoomView) {
  const me = thisScreen(home);
  if (me?.room === r.id) return `You’re here · this ${thisWord} is in the ${r.name}`;
  return roomSummary(r);
}

/** What Home shows in place of rooms while there are none to show. */
function NoRooms({ home }: { home: SmartHome | null }) {
  const set = configured(home);
  return (
    <span style={{ alignSelf: 'center', margin: 'auto', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8, padding: '48px 24px', textAlign: 'center' }}>
      <Icon name="door" size={32} style={{ color: '#5c5c68' }} />
      <span style={{ fontSize: 16, fontWeight: 600, color: '#dedee3' }}>{set ? 'No rooms yet' : 'No devices set up yet'}</span>
      <span style={{ maxWidth: 420, fontSize: 13, lineHeight: 1.45, color: '#8a8a94' }}>
        {set ? 'Edit › New room makes one; then bring devices into it.' : 'Add the lamps, switches and the AC on the TV: Settings › Smart Home.'}
      </span>
    </span>
  );
}

// ---------- Editing the rooms ----------

/** Each room: its icon, name and place in the list, or gone; and a new one. */
function RoomsEdit({ rooms, layout }: { rooms: RoomView[]; layout: Layout }) {
  const [picking, setPicking] = useState<RoomView | null>(null);
  const named = rooms.filter((r) => r.id !== NO_ROOM);
  const phone = layout === 'phone';
  return (
    <>
      <div className="vs" style={{ flex: '1 1 auto', minHeight: 0, display: 'flex', flexDirection: 'column', gap: 10 }}>
        {named.map((r, i) => (
          <span key={r.id} style={{ ...panel, flex: '0 0 auto', display: 'flex', alignItems: 'center', gap: phone ? 8 : 12, minHeight: 72, padding: phone ? '0 10px' : '0 14px' }}>
            <button type="button" className="round" aria-label={`Icon for the ${r.name}`} onClick={() => setPicking(r)} style={{ padding: 0, border: 0, background: 'transparent', cursor: 'pointer' }}>
              <RoomGlyph r={r} size={phone ? 42 : 48} />
            </button>
            <span style={{ flex: '1 1 auto', minWidth: 0, display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
              <span className="ellipsis" style={{ fontSize: 16, fontWeight: 700 }}>
                {r.name}
              </span>
              <span className="ellipsis" style={{ fontSize: 12, color: '#8a8a94' }}>
                {roomSummary(r)}
              </span>
            </span>
            <Round icon="edit" label={`Rename the ${r.name}`} size={40} iconSize={16} onClick={() => askText('Room name', { value: r.name, placeholder: 'Bedroom' }).then((n) => void (n && renameRoom(r.id, n)))} />
            <Round icon="chevronUp" label={`Move the ${r.name} up`} size={40} iconSize={18} disabled={i === 0} onClick={() => shiftRoom(r.id, -1)} />
            <Round icon="chevronDown" label={`Move the ${r.name} down`} size={40} iconSize={18} disabled={i === named.length - 1} onClick={() => shiftRoom(r.id, 1)} />
            <Round
              icon="trash"
              label={`Delete the ${r.name}`}
              size={40}
              iconSize={16}
              color="#ff9a9d"
              onClick={() =>
                askSure(`Delete the ${r.name}?`, 'Its devices stay, with no room; its routines and tasks become the whole house’s.').then((yes) => void (yes && deleteRoom(r.id)))
              }
            />
          </span>
        ))}
        <button
          type="button"
          className="card"
          onClick={() => askText('New room', { placeholder: 'Bedroom, Kitchen, Office…', ok: 'Add' }).then((n) => void (n && addRoom(n, '')))}
          style={{
            flex: '0 0 64px',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 10,
            borderRadius: 22,
            border: '1.5px dashed rgba(255, 255, 255, 0.18)',
            background: 'transparent',
            color: '#c8c8c8',
            fontSize: 15,
            fontWeight: 600,
            cursor: 'pointer',
          }}
        >
          <Icon name="plus" size={18} />
          New room
        </button>
      </div>
      {picking && <IconSheet room={picking.name} current={picking.icon} layout={layout} onPick={(icon) => setRoomIcon(picking.id, icon)} onClose={() => setPicking(null)} />}
    </>
  );
}

// ---------- A room's tiles ----------

/** The room's tiles, in Edit each with ◀ ▶ and its sheet, and a way to bring more in. */
function RoomTiles({ r, editing, layout, shown, answering }: { r: RoomView; editing: boolean; layout: Layout; shown: Shown; answering: (d: Device) => boolean }) {
  const home = useLive((s) => s.home);
  const [sheet, setSheet] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const phone = layout === 'phone';
  if (!home) return <>{Array.from({ length: 4 }, (_, i) => <GhostTile key={i} phone={phone} i={i} />)}</>;
  const hidden = new Set(home.layout.hidden);
  const inTab = r.tiles.filter((d) => (shown === 'online' ? answering(d) : !answering(d)));
  const tiles = editing ? inTab : inTab.filter((d) => !hidden.has(d.id));
  const span = (d: Device) => (phone ? 1 : Math.min(2, tileSpan(d, home.layout)));
  const tile = (d: Device) => (phone ? <PhoneTile d={d} /> : <TabletTile d={d} span={span(d)} />);
  const editingId = sheet ? home.devices.find((d) => d.id === sheet) : undefined;
  return (
    <>
      {tiles.map((d) =>
        editing ? (
          <EditTile key={d.id} d={d} siblings={tiles} hidden={hidden.has(d.id)} span={span(d)} phone={phone} onOpen={() => setSheet(d.id)}>
            {tile(d)}
          </EditTile>
        ) : (
          <span key={d.id} style={{ display: 'contents' }}>
            {tile(d)}
          </span>
        ),
      )}
      {shown === 'offline' && tiles.length === 0 && (
        <span style={{ gridColumn: '1 / -1', padding: '24px 4px', fontSize: 14, color: '#8a8a94' }}>Everything in the {r.name} is answering.</span>
      )}
      {shown === 'online' && (editing || tiles.length === 0) && (
        <button
          type="button"
          className="card"
          onClick={() => setAdding(true)}
          style={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 8,
            padding: 12,
            borderRadius: phone ? 20 : 22,
            border: '1.5px dashed rgba(255, 255, 255, 0.16)',
            background: 'transparent',
            color: '#c8c8c8',
            fontSize: phone ? 13 : 14,
            fontWeight: 600,
            textAlign: 'center',
            cursor: 'pointer',
          }}
        >
          <Icon name="plus" size={phone ? 20 : 24} />
          {tiles.length ? 'Add devices' : `Nothing in the ${r.name} yet · add devices`}
        </button>
      )}
      {editingId && <TileSheet d={editingId} rooms={roomList(home.devices, home.layout)} layout={layout} onClose={() => setSheet(null)} />}
      {adding && <AddDevicesSheet room={r} layout={layout} onClose={() => setAdding(false)} />}
    </>
  );
}

/** Online and Offline, with how many of the room's tiles each has. */
function ShownTabs({ r, shown, setShown, answering, phone }: { r: RoomView; shown: Shown; setShown: (s: Shown) => void; answering: (d: Device) => boolean; phone: boolean }) {
  const home = useLive((s) => s.home);
  const hidden = new Set(home?.layout.hidden ?? []);
  const count = (t: Shown) => r.tiles.filter((d) => !hidden.has(d.id) && (t === 'online' ? answering(d) : !answering(d))).length;
  return (
    <nav
      aria-label="Devices"
      style={
        phone
          ? { position: 'relative', flex: '0 0 36px', marginTop: 8, padding: '0 16px', boxSizing: 'border-box', display: 'flex', alignItems: 'center', gap: 6 }
          : { flex: '0 0 auto', display: 'flex', gap: 2, padding: 4, borderRadius: 999, background: 'rgba(35, 35, 40, 0.72)', border: '1px solid #333333' }
      }
    >
      {(['online', 'offline'] as const).map((t) => {
        const on = shown === t;
        return (
          <button
            key={t}
            type="button"
            className="chip"
            aria-current={on ? 'true' : undefined}
            onClick={() => setShown(t)}
            style={{
              flex: '0 0 auto',
              display: 'flex',
              alignItems: 'center',
              gap: 7,
              height: phone ? 34 : 38,
              padding: '0 14px',
              borderRadius: 999,
              border: phone ? `1px solid ${on ? '#0149B2' : '#333333'}` : 0,
              background: on ? '#0149B2' : phone ? 'rgba(35, 35, 40, 0.8)' : 'transparent',
              color: on ? '#ffffff' : '#a0a0a0',
              fontSize: 13,
              fontWeight: on ? 600 : 500,
              cursor: 'pointer',
              whiteSpace: 'nowrap',
            }}
          >
            {t === 'offline' && <span aria-hidden="true" style={{ width: 7, height: 7, borderRadius: 999, background: '#e5484d' }} />}
            {t === 'online' ? 'Online' : 'Offline'}
            <span style={{ opacity: 0.65 }}>{count(t)}</span>
          </button>
        );
      })}
    </nav>
  );
}

/** The room glows with its first lit light. */
const roomGlow = (r: RoomView) => (r.master.glowFrom && r.master.on.length ? rgba(lightRgb(r.master.glowFrom), 0.18) : rgba([0, 0, 0], 0));

// ---------- Tablet ----------

const TILE_GAP = 12;
/** The tiles' content needs this much (the AC's temperature over its modes and − / +). */
const TILE_MIN = 184;
const TILE_MAX = 280;
const SIDE = 302;

function TabletHome({ room }: { room: string | null }) {
  const { home, rooms, answering } = useHouse();
  const [editing, setEditing] = useState(false);
  const r = room !== null ? rooms.find((x) => x.id === room) : undefined;
  const frame: CSSProperties = { position: 'absolute', top: 10, left: 16, right: 'calc(16px + var(--safe-right))', bottom: 'calc(14px + var(--safe-bottom))', display: 'flex', flexDirection: 'column' };
  return (
    <div style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, overflow: 'hidden' }}>
      {r ? (
        <>
          <div aria-hidden="true" style={glowAt(roomGlow(r), 120, { top: -200, left: 60, width: 720, height: 460 })} />
          <div aria-hidden="true" style={glowAt(rgba([111, 155, 255], 0.08), 120, { top: -160, right: -100, width: 520, height: 400 })} />
          <div style={frame}>
            <TabletRoom key={r.id} r={r} rooms={rooms} home={home} editing={editing} setEditing={setEditing} answering={answering} />
          </div>
        </>
      ) : (
        <>
          <div aria-hidden="true" style={glowAt('rgba(1, 73, 178, 0.2)', 120, { top: -160, left: 160, width: 700, height: 420 })} />
          <div style={frame}>
            <TabletOverview rooms={rooms} home={home} editing={editing} setEditing={setEditing} answering={answering} />
          </div>
        </>
      )}
    </div>
  );
}

interface ViewProps {
  rooms: RoomView[];
  home: SmartHome | null;
  editing: boolean;
  setEditing: (on: boolean) => void;
  answering: (d: Device) => boolean;
}

function TabletOverview({ rooms, home, editing, setEditing }: ViewProps) {
  const [gridRef, grid] = useBox<HTMLDivElement>();
  // Two rows of rooms fill the screen; more rooms scroll.
  const rowH = grid ? Math.max(260, Math.floor((grid.h - 14) / 2)) : 300;
  return (
    <>
      <div style={{ flex: '0 0 48px', display: 'flex', alignItems: 'center', gap: 14, ...fadeUp(0) }}>
        <Title title={editing ? 'Edit rooms' : 'Home'} line={editing ? { text: 'Rename, reorder, pick an icon, add or delete a room', dot: '#4c8dff' } : houseLine(home, rooms)} size={26} />
        {home && configured(home) && <EditButton editing={editing} phone={false} onClick={() => setEditing(!editing)} />}
      </div>
      {editing ? (
        <div style={{ flex: '1 1 0', minHeight: 0, marginTop: 14, display: 'flex', flexDirection: 'column', ...fadeUp(1) }}>
          <RoomsEdit rooms={rooms} layout="tablet" />
        </div>
      ) : (
        <>
          {home && configured(home) && (
            <div style={{ flex: '0 0 52px', marginTop: 14, display: 'flex', gap: 10, minWidth: 0, ...fadeUp(1) }}>
              <Heard home={home} phone={false} style={{ flex: '1 1 auto', height: 52 }} />
              <div className="hs" style={{ flex: '0 1 auto', minWidth: 0, maxWidth: '62%', display: 'flex', gap: 10 }}>
                <HouseRoutines home={home} phone={false} />
              </div>
            </div>
          )}
          <div
            ref={gridRef}
            className="vs"
            style={{
              flex: '1 1 0',
              minHeight: 0,
              marginTop: 14,
              display: rooms.length ? 'grid' : 'flex',
              gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
              gridAutoRows: rowH,
              alignContent: 'start',
              gap: 14,
              ...fadeUp(2),
            }}
          >
            {rooms.length ? rooms.map((r) => <RoomCard key={r.id} r={r} phone={false} onOpen={() => goHome(r.id)} />) : <NoRooms home={home} />}
          </div>
        </>
      )}
    </>
  );
}

function TabletRoom({ r, home, editing, setEditing, answering }: ViewProps & { r: RoomView }) {
  const [shown, setShown] = useState<Shown>('online');
  const [gridRef, grid] = useBox<HTMLElement>();
  const rowH = grid ? Math.max(TILE_MIN, Math.min(TILE_MAX, Math.floor((grid.h - TILE_GAP) / 2))) : 220;
  const lights = r.master.lights.length > 0 && r.id !== NO_ROOM;
  const named = r.id !== NO_ROOM;
  return (
    <>
      <div style={{ flex: '0 0 48px', display: 'flex', alignItems: 'center', gap: 12, ...fadeUp(0) }}>
        <Round icon="chevronLeft" label="All rooms" size={44} iconSize={20} bg="rgba(35, 35, 40, 0.8)" border="1px solid #333333" onClick={() => goHome(null)} />
        <Title title={r.name} line={{ text: editing ? 'Tap a device to rename, move or hide it; ◀ ▶ to reorder' : where(home, r), dot: editing ? '#4c8dff' : r.accent }} size={26} />
        <ShownTabs r={r} shown={shown} setShown={setShown} answering={answering} phone={false} />
        <EditButton editing={editing} phone={false} onClick={() => setEditing(!editing)} />
      </div>
      <div style={{ flex: '1 1 0', minHeight: 0, marginTop: 14, display: 'flex', gap: 14, ...fadeUp(1) }}>
        <div style={{ flex: '1 1 0', minWidth: 0, display: 'flex', flexDirection: 'column', gap: 14 }}>
          {lights && <AllLights r={r} phone={false} style={{ flex: '0 0 auto' }} />}
          <section
            ref={gridRef}
            aria-label="Devices"
            className="vs"
            // Dense: a wide tile that doesn't fit at the end of a row lets the next ones fill the gap.
            style={{ flex: '1 1 0', minHeight: 0, display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gridAutoRows: rowH, gridAutoFlow: 'row dense', alignContent: 'start', gap: TILE_GAP }}
          >
            <RoomTiles r={r} editing={editing} layout="tablet" shown={shown} answering={answering} />
          </section>
        </div>
        <div style={{ flex: `0 0 ${SIDE}px`, minWidth: 0, display: 'flex', flexDirection: 'column', gap: 14 }}>
          {named ? (
            <>
              <Routines r={r} editing={editing} style={{ flex: '1 1 0' }} />
              <Tasks room={r.id} tasks={r.tasks} accent={r.accent} editing={editing} style={{ flex: '1 1 0' }} />
              <Screens r={r} editing={editing} phone={false} style={{ flex: '0 0 auto' }} />
            </>
          ) : (
            <span style={{ ...panel, padding: '16px 18px', fontSize: 13, lineHeight: 1.5, color: '#a0a0a0' }}>
              These devices aren’t in a room yet. Edit, tap one and pick its room: it moves there, with its tile.
            </span>
          )}
        </div>
      </div>
    </>
  );
}

// ---------- Phone ----------

function PhoneHome({ room }: { room: string | null }) {
  const { home, rooms, answering } = useHouse();
  const [editing, setEditing] = useState(false);
  const r = room !== null ? rooms.find((x) => x.id === room) : undefined;
  return (
    <div style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
      <div aria-hidden="true" style={glowAt(r ? roomGlow(r) : 'rgba(1, 73, 178, 0.24)', 110, { top: -140, left: -60, width: 480, height: 360 })} />
      {r ? (
        <PhoneRoom key={r.id} r={r} rooms={rooms} home={home} editing={editing} setEditing={setEditing} answering={answering} />
      ) : (
        <PhoneOverview rooms={rooms} home={home} editing={editing} setEditing={setEditing} answering={answering} />
      )}
    </div>
  );
}

function PhoneOverview({ rooms, home, editing, setEditing }: ViewProps) {
  return (
    <>
      <div style={{ position: 'relative', flex: '0 0 52px', margin: '0 16px', display: 'flex', alignItems: 'center', gap: 12, ...fadeUp(0) }}>
        <Title title={editing ? 'Edit rooms' : 'Home'} line={editing ? { text: 'Rename, reorder, add or delete', dot: '#4c8dff' } : houseLine(home, rooms)} size={24} />
        {home && configured(home) && <EditButton editing={editing} phone onClick={() => setEditing(!editing)} />}
      </div>
      {editing ? (
        <div style={{ position: 'relative', flex: '1 1 auto', minHeight: 0, margin: '10px 16px 12px', display: 'flex', flexDirection: 'column' }}>
          <RoomsEdit rooms={rooms} layout="phone" />
        </div>
      ) : (
        <>
          {home && configured(home) && (
            <>
              <Heard home={home} phone style={{ position: 'relative', flex: '0 0 54px', margin: '10px 16px 0' }} />
              <div className="hs" style={{ position: 'relative', flex: '0 0 44px', marginTop: 10, padding: '0 16px', boxSizing: 'border-box', display: 'flex', alignItems: 'center', gap: 8, ...fadeUp(1) }}>
                <HouseRoutines home={home} phone />
                <span aria-hidden="true" style={{ flex: '0 0 8px', height: 1 }} />
              </div>
            </>
          )}
          <div className="vs" style={{ position: 'relative', flex: '1 1 auto', minHeight: 0, margin: '10px 16px 12px', display: 'flex', flexDirection: 'column', gap: 10, ...fadeUp(2) }}>
            {rooms.length ? rooms.map((r) => <RoomCard key={r.id} r={r} phone onOpen={() => goHome(r.id)} style={{ flex: '0 0 112px' }} />) : <NoRooms home={home} />}
          </div>
        </>
      )}
    </>
  );
}

type PhoneTab = 'devices' | 'routines' | 'tasks' | 'screens';
const PHONE_TABS: [PhoneTab, string][] = [
  ['devices', 'Devices'],
  ['routines', 'Routines'],
  ['tasks', 'Tasks'],
  ['screens', 'Screens'],
];

function PhoneRoom({ r, home, editing, setEditing, answering }: ViewProps & { r: RoomView }) {
  const [shown, setShown] = useState<Shown>('online');
  const [tab, setTab] = useState<PhoneTab>('devices');
  const lights = r.master.lights.length > 0 && r.id !== NO_ROOM;
  const tabs = r.id === NO_ROOM ? PHONE_TABS.slice(0, 1) : PHONE_TABS;
  const show = tabs.some(([t]) => t === tab) ? tab : 'devices';
  return (
    <>
      <div style={{ position: 'relative', flex: '0 0 50px', margin: '0 16px', display: 'flex', alignItems: 'center', gap: 10, ...fadeUp(0) }}>
        <Round icon="chevronLeft" label="All rooms" size={40} iconSize={18} bg="rgba(35, 35, 40, 0.8)" border="1px solid #333333" onClick={() => goHome(null)} />
        <Title title={r.name} line={{ text: editing ? 'Tap a device to rename, move or hide it' : where(home, r), dot: editing ? '#4c8dff' : r.accent }} size={22} />
        <EditButton editing={editing} phone onClick={() => setEditing(!editing)} />
      </div>
      {(show === 'devices' || editing) && <ShownTabs r={r} shown={shown} setShown={setShown} answering={answering} phone />}
      {lights && !editing && <AllLights r={r} phone style={{ flex: '0 0 auto', margin: '10px 16px 0' }} />}
      {tabs.length > 1 && (
        <div
          role="tablist"
          aria-label="In this room"
          style={{
            position: 'relative',
            flex: '0 0 40px',
            margin: '10px 16px 0',
            display: 'flex',
            gap: 2,
            padding: 3,
            boxSizing: 'border-box',
            borderRadius: 999,
            background: 'rgba(30, 30, 38, 0.86)',
            border: '1px solid rgba(255, 255, 255, 0.07)',
          }}
        >
          {tabs.map(([t, label]) => {
            const on = show === t;
            return (
              <button
                key={t}
                type="button"
                role="tab"
                className="chip"
                aria-selected={on}
                onClick={() => setTab(t)}
                style={{ flex: '1 1 0', minWidth: 0, height: 32, padding: 0, border: 0, borderRadius: 999, background: on ? '#0149B2' : 'transparent', color: on ? '#ffffff' : '#a0a0a0', fontSize: 12, fontWeight: 600, cursor: 'pointer' }}
              >
                {label}
              </button>
            );
          })}
        </div>
      )}
      <div style={{ position: 'relative', flex: '1 1 auto', minHeight: 0, margin: '10px 16px 12px', display: 'flex', flexDirection: 'column' }}>
        {show === 'devices' && (
          <section
            aria-label="Devices"
            className="vs"
            style={{ flex: '1 1 auto', minHeight: 0, display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gridAutoRows: 122, gridAutoFlow: 'row dense', alignContent: 'start', gap: 10 }}
          >
            <RoomTiles r={r} editing={editing} layout="phone" shown={shown} answering={answering} />
          </section>
        )}
        {show === 'routines' && <Routines r={r} editing={editing} flat style={{ flex: '1 1 auto' }} />}
        {show === 'tasks' && <Tasks room={r.id} tasks={r.tasks} accent={r.accent} editing={editing} flat style={{ flex: '1 1 auto' }} />}
        {show === 'screens' && <Screens r={r} editing={editing} phone flat />}
      </div>
    </>
  );
}
