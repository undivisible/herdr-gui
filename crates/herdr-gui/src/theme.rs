#[derive(Clone, Copy)]
pub struct UiTheme {
    pub bg: u32,
    pub panel: u32,
    pub terminal: u32,
    pub text: u32,
    pub label: u32,
    pub muted: u32,
    pub hover: u32,
    pub active: u32,
    pub border: u32,
}

pub fn agent_status_background(status: &str, theme: UiTheme) -> u32 {
    let light = theme.bg > 0x808080;
    match status {
        "working" => {
            if light {
                0xfff3d6
            } else {
                0x2a210f
            }
        }
        "blocked" => {
            if light {
                0xffe1e1
            } else {
                0x2b1515
            }
        }
        "done" => {
            if light {
                0xdff6e6
            } else {
                0x102615
            }
        }
        _ => theme.panel,
    }
}

pub fn agent_status_accent(status: &str) -> u32 {
    match status {
        "working" => 0xf59e0b,
        "blocked" => 0xef4444,
        "done" => 0x22c55e,
        _ => 0x8a8a8a,
    }
}

pub fn status_color(status: &str) -> u32 {
    match status {
        "working" => 0xf59e0b,
        "blocked" => 0xef4444,
        "done" => 0x22c55e,
        "idle" => 0x8a8a8a,
        _ => 0x555555,
    }
}

pub fn herdr_theme(name: &str) -> UiTheme {
    match name {
        "catppuccin-latte" => theme(
            0xf5f5f5, 0xeff1f5, 0xffffff, 0x4c4f69, 0x6c6f85, 0xe6e9ef, 0xccd0da,
        ),
        "terminal" => theme(
            0x0b0b0b, 0x0b0b0b, 0x080808, 0xf2f2f2, 0x8a8a8a, 0x181818, 0x202020,
        ),
        "tokyo-night" => theme(
            0x1a1b26, 0x1a1b26, 0x11121a, 0xc0caf5, 0xa9b1d6, 0x24283b, 0x414868,
        ),
        "tokyo-night-day" => theme(
            0xe1e2e7, 0xe1e2e7, 0xf8f8fb, 0x3760bf, 0x6172b0, 0xd2d3da, 0xc4c8da,
        ),
        "dracula" => theme(
            0x282a36, 0x282a36, 0x15161c, 0xf8f8f2, 0xd2d2dc, 0x44475a, 0x6272a4,
        ),
        "nord" => theme(
            0x2e3440, 0x2e3440, 0x20242d, 0xeceff4, 0xd8dee9, 0x3b4252, 0x434c5e,
        ),
        "gruvbox" => theme(
            0x282828, 0x282828, 0x1d2021, 0xebdbb2, 0xd5c4a1, 0x3c3836, 0x504945,
        ),
        "gruvbox-light" => theme(
            0xfbf1c7, 0xfbf1c7, 0xfffff0, 0x3c3836, 0x504945, 0xf2e5bc, 0xebdbb2,
        ),
        "one-dark" => theme(
            0x282c34, 0x282c34, 0x1f2329, 0xabb2bf, 0x969ca8, 0x2c313a, 0x3e4451,
        ),
        "one-light" => theme(
            0xfafafa, 0xfafafa, 0xffffff, 0x383a42, 0x686b77, 0xf5f5f6, 0xe5e5e6,
        ),
        "solarized" => theme(
            0x002b36, 0x002b36, 0x001f27, 0x93a1a1, 0x839496, 0x073642, 0x586e75,
        ),
        "solarized-light" => theme(
            0xfdf6e3, 0xfdf6e3, 0xfffff4, 0x657b83, 0x839496, 0xeee8d5, 0x93a1a1,
        ),
        "kanagawa" => theme(
            0x1f1f28, 0x1f1f28, 0x16161d, 0xdcd7ba, 0xc8c3aa, 0x2a2a37, 0x363646,
        ),
        "kanagawa-lotus" => theme(
            0xf2ecbc, 0xf2ecbc, 0xfffae0, 0x545464, 0x43436c, 0xd5cea3, 0xdcd5ac,
        ),
        "rose-pine" => theme(
            0x191724, 0x191724, 0x111019, 0xe0def4, 0xc8c5dc, 0x1f1d2e, 0x26233a,
        ),
        "rose-pine-dawn" => theme(
            0xfaf4ed, 0xfaf4ed, 0xfffbf5, 0x464261, 0x797593, 0xf2e9e1, 0xfffaf3,
        ),
        "vesper" => theme(
            0x1a1a1a, 0x1a1a1a, 0x101010, 0xffffff, 0xa0a0a0, 0x232323, 0x282828,
        ),
        _ => theme(
            0x181825, 0x181825, 0x11111b, 0xcdd6f4, 0xa6adc8, 0x1e1e2e, 0x313244,
        ),
    }
}

fn theme(
    bg: u32,
    panel: u32,
    terminal: u32,
    text: u32,
    muted: u32,
    hover: u32,
    active: u32,
) -> UiTheme {
    UiTheme {
        bg,
        panel,
        terminal,
        text,
        label: text,
        muted,
        hover,
        active,
        border: active,
    }
}
