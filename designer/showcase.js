const ink = "#11131A";
const panel = "#1B1F2A";
const panelRaised = "#252B38";
const ember = "#D88A5B";
const emberBright = "#F0A46F";
const cream = "#F4E9D8";
const muted = "#A89F94";
const mint = "#72C7A5";

function text(parent, runtimeId, content, size, weight, color, extra = {}) {
  return I(parent, {
    type: "text",
    name: content,
    runtimeId,
    content,
    fontFamily: "Inter",
    fontSize: size,
    fontWeight: weight,
    fill: [{ type: "solid", color }],
    width: "fit_content",
    height: "fit_content",
    ...extra,
  });
}

function button(parent, runtimeId, label, tabIndex, width = 260) {
  const states = [
    ["default", panelRaised, cream, "#3C4556", 1],
    ["hover", "#333B4B", cream, ember, 2],
    ["pressed", "#8E5638", "#FFF7EC", emberBright, 2],
    ["disabled", "#242731", "#6F727B", "#343842", 1],
    ["focused", panelRaised, cream, emberBright, 3],
  ];
  const visualStates = {};
  for (const [state] of states) visualStates[state] = `${runtimeId}.${state}`;
  const control = I(parent, {
    type: "frame",
    name: label,
    runtimeId,
    role: "button",
    accessibilityLabel: label,
    tabIndex,
    visualStates,
    width,
    height: 52,
    cornerRadius: 12,
  });
  for (const [state, fill, foreground, border, thickness] of states) {
    const layer = I(control, {
      type: "frame",
      name: `${label} ${state}`,
      runtimeId: `${runtimeId}.${state}`,
      x: 0,
      y: 0,
      width: "fill_container",
      height: "fill_container",
      cornerRadius: 12,
      layout: "horizontal",
      justifyContent: "center",
      alignItems: "center",
      fill: [{ type: "solid", color: fill }],
      stroke: { thickness, fill: [{ type: "solid", color: border }] },
    });
    text(layer, `${runtimeId}.${state}.label`, label, 17, 650, foreground, {
      name: `${label} ${state} label`,
    });
  }
  return control;
}

const app = I(null, {
  type: "frame",
  name: "Regicide Interface",
  runtimeId: "app.root",
  width: 1280,
  height: 720,
  fill: [{ type: "solid", color: ink }],
});

const menu = I(app, {
  type: "frame",
  name: "Main Menu",
  runtimeId: "screen.main_menu",
  x: 0,
  y: 0,
  width: "fill_container",
  height: "fill_container",
  layout: "horizontal",
  fill: [{ type: "solid", color: ink }],
});
const menuRail = I(menu, {
  type: "frame",
  name: "Menu Rail",
  runtimeId: "main_menu.rail",
  width: 430,
  height: "fill_container",
  layout: "vertical",
  justifyContent: "center",
  alignItems: "center",
  gap: 18,
  padding: 48,
  fill: [{ type: "solid", color: panel }],
});
text(menuRail, "main_menu.eyebrow", "A THRONE AWAITS", 13, 700, ember, { letterSpacing: 2 });
text(menuRail, "main_menu.title", "REGICIDE", 52, 850, cream);
text(menuRail, "main_menu.subtitle", "Tactical duels. No sovereigns.", 16, 450, muted);
button(menuRail, "main_menu.play", "Play", 0);
button(menuRail, "main_menu.settings", "Settings", 1);
button(menuRail, "main_menu.quit", "Quit", 2);

const menuField = I(menu, {
  type: "frame",
  name: "Campaign Field",
  runtimeId: "main_menu.field",
  width: "fill_container",
  height: "fill_container",
  layout: "vertical",
  justifyContent: "space_between",
  padding: 52,
  fill: [{ type: "solid", color: "#151924" }],
});
text(menuField, "main_menu.chapter", "CHAPTER VII", 14, 750, ember, { letterSpacing: 2 });
const quote = I(menuField, {
  type: "frame",
  name: "Field Note",
  runtimeId: "main_menu.note",
  width: "fill_container",
  height: 220,
  layout: "vertical",
  gap: 14,
  padding: 30,
  cornerRadius: 18,
  fill: [{ type: "solid", color: panelRaised }],
  stroke: { thickness: 1, fill: [{ type: "solid", color: "#394153" }] },
});
text(quote, "main_menu.note.title", "The crown is only a target.", 30, 700, cream);
text(quote, "main_menu.note.body", "Break the line, guard the king, and leave no move unanswered.", 16, 450, muted);
text(menuField, "main_menu.build", "DESIGN SOURCE: OPENPENCIL  /  RUNTIME: BEVY 0.19", 12, 600, muted);

const settings = I(app, {
  type: "frame",
  name: "Settings Screen",
  runtimeId: "screen.settings",
  visible: false,
  x: 0,
  y: 0,
  width: "fill_container",
  height: "fill_container",
  layout: "vertical",
  alignItems: "center",
  gap: 22,
  padding: 40,
  fill: [{ type: "solid", color: ink }],
});
text(settings, "settings.title", "SETTINGS", 38, 800, cream);
text(settings, "settings.caption", "Application-owned values, designer-owned presentation", 15, 450, muted);
const settingsCard = I(settings, {
  type: "frame",
  name: "Settings Card",
  runtimeId: "settings.card",
  width: "fill_container",
  height: 390,
  layout: "vertical",
  gap: 18,
  padding: 30,
  cornerRadius: 18,
  fill: [{ type: "solid", color: panel }],
  stroke: { thickness: 1, fill: [{ type: "solid", color: "#394153" }] },
});
text(settingsCard, "settings.section", "DISPLAY & AUDIO", 13, 750, ember, { letterSpacing: 2 });
text(settingsCard, "settings.fullscreen", "Fullscreen: On", 20, 600, cream);
button(settingsCard, "settings.toggle_fullscreen", "Toggle Fullscreen", 3, 330);
text(settingsCard, "settings.music", "Music Volume: 70%", 20, 600, cream);
const volumeRow = I(settingsCard, {
  type: "frame",
  name: "Volume Controls",
  runtimeId: "settings.volume_controls",
  width: "fill_container",
  height: 52,
  layout: "horizontal",
  gap: 14,
});
button(volumeRow, "settings.music_down", "Volume -", 4, 180);
button(volumeRow, "settings.music_up", "Volume +", 5, 180);
const settingsActions = I(settings, {
  type: "frame",
  name: "Settings Actions",
  runtimeId: "settings.actions",
  width: "fill_container",
  height: 52,
  layout: "horizontal",
  justifyContent: "space_between",
});
button(settingsActions, "settings.back", "Back", 6, 220);
button(settingsActions, "settings.apply", "Apply", 7, 220);

const hud = I(app, {
  type: "frame",
  name: "Battle HUD",
  runtimeId: "screen.hud",
  visible: false,
  x: 0,
  y: 0,
  width: "fill_container",
  height: "fill_container",
  layout: "vertical",
  justifyContent: "space_between",
  padding: 30,
  fill: [{ type: "solid", color: "#171B24" }],
});
const hudTop = I(hud, {
  type: "frame",
  name: "HUD Top Bar",
  runtimeId: "hud.top",
  width: "fill_container",
  height: 76,
  layout: "horizontal",
  alignItems: "center",
  justifyContent: "space_between",
  padding: 18,
  cornerRadius: 14,
  fill: [{ type: "solid", color: panel }],
});
text(hudTop, "hud.player_name", "Player: Rowan", 20, 650, cream);
text(hudTop, "hud.score", "Score: 1200", 20, 700, emberBright);
button(hudTop, "hud.pause", "Pause", 8, 150);
const battlefield = I(hud, {
  type: "frame",
  name: "Battlefield",
  runtimeId: "hud.battlefield",
  width: "fill_container",
  height: "fill_container",
  layout: "vertical",
  alignItems: "center",
  justifyContent: "center",
  gap: 16,
});
text(battlefield, "hud.turn", "YOUR TURN", 16, 800, ember, { letterSpacing: 3 });
text(battlefield, "hud.objective", "Hold the center. Pressure the crown.", 30, 650, cream);
const health = I(hud, {
  type: "frame",
  name: "Health Panel",
  runtimeId: "hud.health_panel",
  width: "fill_container",
  height: 70,
  layout: "horizontal",
  alignItems: "center",
  justifyContent: "space_between",
  padding: 18,
  cornerRadius: 14,
  fill: [{ type: "solid", color: panel }],
});
text(health, "hud.health", "King Health: 84", 20, 700, mint);
text(health, "hud.status", "Formation stable", 16, 500, muted);

const pause = I(app, {
  type: "frame",
  name: "Pause Menu",
  runtimeId: "screen.pause",
  visible: false,
  x: 0,
  y: 0,
  width: "fill_container",
  height: "fill_container",
  layout: "vertical",
  alignItems: "center",
  justifyContent: "center",
  gap: 18,
  fill: [{ type: "solid", color: "#11131AE6" }],
});
text(pause, "pause.title", "PAUSED", 42, 800, cream);
button(pause, "pause.resume", "Resume", 9);
button(pause, "pause.menu", "Return to Menu", 10);
