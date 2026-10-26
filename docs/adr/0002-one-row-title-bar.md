# One-row title bar: icon, menus, tabs and window buttons

scrivi draws its own title bar: one 42 px row holding the app icon, File/Edit/View, the tabs, a drag area and the minimise/maximise/close buttons, as Windows 11 Notepad does. The native caption, menu bar and tab strip together took 96 px above the text, which Mattia found wasteful. We accept owning the caption work: hit-testing for drag, double-click to maximise, resize edges and the Snap Layouts flyout on maximise. The menus are native popup menus opened from buttons we draw, so Alt and access keys have to be wired up by hand.

## Considered Options

- **Native caption, menu bar and tab row (96 px)**: zero caption code, but too much vertical space.
- **Menus in the tab row under the native caption (70 px)**: cheap, but saves only one row.

## Consequences

- There is no title text: the file name is on the tab and the full path in its tooltip. The window title is still set for Alt+Tab and the taskbar.
