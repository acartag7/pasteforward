import React from 'react';
import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';

const colors = {
  bg: '#f7f4ec',
  panel: '#fffdf8',
  ink: '#17191d',
  muted: '#67707d',
  line: '#d9d3c5',
  blue: '#1f6feb',
  green: '#177245',
  amber: '#b86e00',
  red: '#b42318',
  terminal: '#101317',
  terminalInk: '#d6f5df',
};

const container: React.CSSProperties = {
  background: colors.bg,
  color: colors.ink,
  fontFamily:
    'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
};

const mono: React.CSSProperties = {
  fontFamily:
    '"SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace',
};

const fade = (frame: number, start: number, end: number) =>
  interpolate(frame, [start, end], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  });

const fadeOut = (frame: number, start: number, end: number) =>
  interpolate(frame, [start, end], [1, 0], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.in(Easing.cubic),
  });

const slide = (frame: number, start: number, from: number, to = 0) =>
  interpolate(frame, [start, start + 18], [from, to], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing: Easing.out(Easing.cubic),
  });

const Terminal = ({
  title,
  lines,
  opacity = 1,
  style,
}: {
  title: string;
  lines: string[];
  opacity?: number;
  style?: React.CSSProperties;
}) => (
  <div
    style={{
      background: colors.terminal,
      border: `1px solid ${colors.ink}`,
      borderRadius: 8,
      boxShadow: '0 18px 40px rgba(23, 25, 29, 0.18)',
      color: colors.terminalInk,
      overflow: 'hidden',
      opacity,
      ...style,
    }}
  >
    <div
      style={{
        alignItems: 'center',
        background: '#1b2027',
        borderBottom: '1px solid #303842',
        color: '#c9d1d9',
        display: 'flex',
        fontSize: 13,
        height: 34,
        justifyContent: 'space-between',
        padding: '0 14px',
      }}
    >
      <span style={{...mono, fontWeight: 700}}>{title}</span>
      <span style={{color: '#768390'}}>ssh</span>
    </div>
    <div style={{...mono, fontSize: 18, lineHeight: 1.55, padding: 18}}>
      {lines.map((line, index) => (
        <div
          key={`${line}-${index}`}
          style={{
            color: line.startsWith('>') ? '#f0f6fc' : colors.terminalInk,
            opacity: line.includes('[waiting]') ? 0.78 : 1,
            whiteSpace: 'pre',
          }}
        >
          {line}
        </div>
      ))}
    </div>
  </div>
);

const StatusPill = ({
  label,
  tone,
}: {
  label: string;
  tone: 'green' | 'amber' | 'blue';
}) => {
  const palette = {
    green: ['#e6f4ea', colors.green],
    amber: ['#fff2d6', colors.amber],
    blue: ['#e8f0fe', colors.blue],
  }[tone];

  return (
    <span
      style={{
        ...mono,
        background: palette[0],
        border: `1px solid ${palette[1]}33`,
        borderRadius: 999,
        color: palette[1],
        fontSize: 14,
        fontWeight: 800,
        letterSpacing: 0,
        padding: '6px 10px',
      }}
    >
      {label}
    </span>
  );
};

const Destination = ({
  name,
  host,
  mode,
  active,
  delay,
}: {
  name: string;
  host: string;
  mode: string;
  active: boolean;
  delay: number;
}) => {
  const frame = useCurrentFrame();
  const pop = spring({
    frame: frame - delay,
    fps: 24,
    config: {damping: 16, stiffness: 130, mass: 0.7},
  });

  return (
    <div
      style={{
        alignItems: 'center',
        background: colors.panel,
        border: `1px solid ${active ? colors.green : colors.line}`,
        borderRadius: 8,
        boxShadow: active
          ? '0 12px 26px rgba(23, 114, 69, 0.16)'
          : '0 10px 20px rgba(23, 25, 29, 0.08)',
        display: 'grid',
        gridTemplateColumns: '42px 1fr auto',
        gap: 10,
        minHeight: 78,
        opacity: active ? 1 : 0.84,
        padding: 14,
        transform: `scale(${0.98 + pop * 0.02})`,
      }}
    >
      <div
        style={{
          alignItems: 'center',
          background: active ? colors.green : '#ece8df',
          borderRadius: 8,
          color: active ? '#ffffff' : colors.muted,
          display: 'flex',
          fontSize: 15,
          fontWeight: 900,
          height: 42,
          justifyContent: 'center',
          width: 42,
        }}
      >
        {name.slice(0, 2).toUpperCase()}
      </div>
      <div>
        <div style={{fontSize: 18, fontWeight: 900, lineHeight: 1.1}}>
          {name}
        </div>
        <div
          style={{
            ...mono,
            color: colors.muted,
            fontSize: 12,
            marginTop: 5,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {host}
        </div>
      </div>
      <StatusPill
        label={active ? 'READY' : mode.toUpperCase()}
        tone={active ? 'green' : 'blue'}
      />
    </div>
  );
};

const ClipboardCard = ({progress}: {progress: number}) => (
  <div
    style={{
      alignItems: 'center',
      background: colors.panel,
      border: `1px solid ${colors.line}`,
      borderRadius: 8,
      boxShadow: '0 18px 40px rgba(23, 25, 29, 0.12)',
      display: 'grid',
      gap: 14,
      gridTemplateColumns: '92px 1fr',
      minHeight: 132,
      padding: 18,
      width: 390,
    }}
  >
    <div
      style={{
        background:
          'linear-gradient(135deg, #ffffff 0%, #e7f0fb 44%, #dff4e9 100%)',
        border: `2px solid ${colors.blue}`,
        borderRadius: 8,
        height: 92,
        overflow: 'hidden',
        position: 'relative',
        width: 92,
      }}
    >
      <div
        style={{
          background: colors.green,
          bottom: 12,
          height: 24,
          left: 14,
          opacity: 0.88,
          position: 'absolute',
          transform: `translateY(${(1 - progress) * 18}px)`,
          width: 58,
        }}
      />
      <div
        style={{
          background: colors.amber,
          height: 26,
          opacity: 0.86,
          position: 'absolute',
          right: 12,
          top: 15,
          transform: `translateY(${(1 - progress) * -14}px)`,
          width: 34,
        }}
      />
      <div
        style={{
          ...mono,
          alignItems: 'center',
          background: '#ffffffcc',
          bottom: 0,
          color: colors.ink,
          display: 'flex',
          fontSize: 15,
          fontWeight: 900,
          height: 28,
          justifyContent: 'center',
          position: 'absolute',
          width: '100%',
        }}
      >
        PNG
      </div>
    </div>
    <div>
      <div style={{fontSize: 23, fontWeight: 950, lineHeight: 1}}>
        Local image clipboard
      </div>
      <div style={{color: colors.muted, fontSize: 15, marginTop: 9}}>
        PasteForward watches image-only changes and fans them out over SSH.
      </div>
      <div style={{display: 'flex', gap: 8, marginTop: 14}}>
        <StatusPill label="NO HISTORY" tone="amber" />
        <StatusPill label="TTL /tmp" tone="blue" />
      </div>
    </div>
  </div>
);

const FlowLine = ({
  x,
  y,
  width,
  progress,
  label,
  opacity = 1,
}: {
  x: number;
  y: number;
  width: number;
  progress: number;
  label: string;
  opacity?: number;
}) => (
  <div style={{left: x, opacity, position: 'absolute', top: y, width}}>
    <div
      style={{
        background: colors.line,
        borderRadius: 999,
        height: 6,
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          background: colors.blue,
          borderRadius: 999,
          height: '100%',
          width: `${progress * 100}%`,
        }}
      />
    </div>
    {label ? (
      <div
        style={{
          ...mono,
          color: colors.blue,
          fontSize: 12,
          fontWeight: 900,
          marginTop: 8,
          opacity: progress > 0.12 ? 1 : 0,
          textAlign: 'center',
        }}
      >
        {label}
      </div>
    ) : null}
  </div>
);

const Header = () => (
  <div style={{left: 54, position: 'absolute', top: 36}}>
    <div
      style={{
        ...mono,
        color: colors.blue,
        fontSize: 15,
        fontWeight: 900,
        marginBottom: 9,
      }}
    >
      PASTEFORWARD
    </div>
    <div style={{fontSize: 42, fontWeight: 950, letterSpacing: 0, lineHeight: 1}}>
      Image paste over SSH
    </div>
    <div style={{color: colors.muted, fontSize: 19, marginTop: 8}}>
      Local clipboard in. Remote Claude Code and Codex paste path out.
    </div>
  </div>
);

const IntroScene = ({frame}: {frame: number}) => {
  const terminalOpacity = fade(frame, 24, 44);
  const warningOpacity = fade(frame, 66, 84) * fadeOut(frame, 118, 140);
  const terminalY = slide(frame, 26, 24);

  return (
    <AbsoluteFill style={{...container, opacity: fadeOut(frame, 145, 168)}}>
      <Header />
      <Terminal
        title="ssh macmini -- claude"
        opacity={terminalOpacity}
        lines={[
          '> paste image',
          '[waiting] paste image into this terminal',
          '',
          'clipboard payload: text only',
        ]}
        style={{
          height: 250,
          left: 64,
          position: 'absolute',
          top: 214 + terminalY,
          width: 832,
        }}
      />
      <div
        style={{
          alignItems: 'center',
          background: '#fff5f5',
          border: `1px solid ${colors.red}55`,
          borderRadius: 8,
          bottom: 54,
          boxShadow: '0 14px 28px rgba(180, 35, 24, 0.12)',
          color: colors.red,
          display: 'flex',
          fontSize: 22,
          fontWeight: 900,
          gap: 14,
          left: 183,
          opacity: warningOpacity,
          padding: '16px 22px',
          position: 'absolute',
        }}
      >
        <span
          style={{
            ...mono,
            alignItems: 'center',
            border: `2px solid ${colors.red}`,
            borderRadius: 999,
            display: 'flex',
            height: 32,
            justifyContent: 'center',
            width: 32,
          }}
        >
          !
        </span>
        Terminal SSH does not carry the desktop image clipboard.
      </div>
    </AbsoluteFill>
  );
};

const BridgeScene = ({frame}: {frame: number}) => {
  const localIn = fade(frame, 142, 164);
  const progress = fade(frame, 178, 226);
  const destinations = fade(frame, 198, 226);
  const terminal = fade(frame, 232, 252);
  const history = fade(frame, 264, 286);
  const flowOpacity = fadeOut(frame, 232, 254);
  const destinationOpacity = destinations * fadeOut(frame, 246, 266);

  return (
    <AbsoluteFill
      style={{
        ...container,
        opacity: fade(frame, 150, 170),
      }}
    >
      <Header />
      <div
        style={{
          left: 54,
          opacity: localIn * flowOpacity,
          position: 'absolute',
          top: 186,
          transform: `translateX(${slide(frame, 148, -28)}px)`,
        }}
      >
        <ClipboardCard progress={progress} />
      </div>
      <div
        style={{
          ...mono,
          alignItems: 'center',
          background: colors.ink,
          borderRadius: 8,
          color: '#ffffff',
          display: 'flex',
          fontSize: 19,
          fontWeight: 900,
          height: 76,
          justifyContent: 'center',
          left: 456,
          opacity: fade(frame, 166, 188) * flowOpacity,
          position: 'absolute',
          top: 214,
          width: 124,
        }}
      >
        daemon
      </div>
      <FlowLine x={444} y={249} width={16} progress={progress} label="" opacity={flowOpacity} />
      <FlowLine x={578} y={249} width={112} progress={progress} label="ssh" opacity={flowOpacity} />
      <div
        style={{
          display: 'grid',
          gap: 14,
          left: 690,
          opacity: destinationOpacity,
          position: 'absolute',
          top: 172,
          width: 220,
        }}
      >
        <Destination
          name="macmini"
          host="user@mac.example"
          mode="macos"
          active={progress > 0.84}
          delay={224}
        />
        <Destination
          name="devvm"
          host="dev@linux-vm"
          mode="wayland"
          active={progress > 0.92}
          delay={236}
        />
      </div>
      <Terminal
        title="remote agent"
        opacity={terminal}
        lines={[
          '> paste',
          '[Image #1]',
          '',
          'pasteforward status macmini',
          'OK image forwarded 4s ago',
        ]}
        style={{
          bottom: 38,
          height: 160,
          left: 54,
          position: 'absolute',
          width: 566,
        }}
      />
      <div
        style={{
          background: colors.panel,
          border: `1px solid ${colors.line}`,
          borderRadius: 8,
          bottom: 38,
          boxShadow: '0 12px 24px rgba(23, 25, 29, 0.08)',
          opacity: history,
          padding: 18,
          position: 'absolute',
          right: 48,
          width: 278,
        }}
      >
        <div style={{fontSize: 21, fontWeight: 950}}>Image-only history</div>
        <div style={{color: colors.muted, fontSize: 15, lineHeight: 1.35, marginTop: 8}}>
          Metadata by default. Image bytes only when explicitly enabled.
        </div>
        <div style={{display: 'grid', gap: 8, marginTop: 14}}>
          <StatusPill label="doctor checks commands" tone="green" />
          <StatusPill label="cleanup purges cache" tone="blue" />
        </div>
      </div>
    </AbsoluteFill>
  );
};

export const PasteForwardDemo = () => {
  const frame = useCurrentFrame();
  const {durationInFrames} = useVideoConfig();
  const outro = fade(frame, durationInFrames - 38, durationInFrames - 12);

  return (
    <AbsoluteFill style={container}>
      <IntroScene frame={frame} />
      <BridgeScene frame={frame} />
      <div
        style={{
          ...mono,
          background: colors.ink,
          borderRadius: 8,
          color: '#ffffff',
          fontSize: 16,
          fontWeight: 900,
          opacity: outro,
          padding: '10px 14px',
          position: 'absolute',
          right: 34,
          top: 154,
        }}
      >
        pasteforward ssh macmini -- claude
      </div>
    </AbsoluteFill>
  );
};
