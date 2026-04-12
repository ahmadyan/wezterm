//! Standalone test of the WezTerm terminal core + font config.
//!
//! This tests the non-GPU parts of libwezterm:
//! - Creating a terminal
//! - Feeding bytes
//! - Reading screen state
//! - Creating a FontConfiguration
//! - Getting font metrics
//!
//! Run with: cargo run -p libwezterm --example test_terminal

use std::io::Write;
use std::sync::Arc;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    println!("=== WezTerm Standalone Test ===\n");

    // Test 1: Create a terminal
    println!("Test 1: Creating terminal...");
    test_terminal_creation();

    // Test 2: Create a font configuration
    println!("\nTest 2: Creating FontConfiguration...");
    test_font_config();

    // Test 3: Feed bytes and read screen
    println!("\nTest 3: Feed bytes and read screen...");
    test_feed_and_read();

    println!("\n=== All tests passed ===");
}

struct TestWriter;
impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn test_terminal_creation() {
    use wezterm_term::config::TerminalConfiguration;
    use wezterm_term::terminal::Terminal;
    use wezterm_term::TerminalSize;

    #[derive(Debug)]
    struct TestConfig;
    impl TerminalConfiguration for TestConfig {
        fn color_palette(&self) -> wezterm_term::color::ColorPalette {
            wezterm_term::color::ColorPalette::default()
        }
    }

    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 640,
        pixel_height: 384,
        dpi: 96,
    };

    let config: Arc<dyn TerminalConfiguration + Send + Sync> = Arc::new(TestConfig);
    let _terminal = Terminal::new(
        size,
        config,
        "Agentastic-Test",
        "0.1",
        Box::new(TestWriter),
    );
    println!("  ✓ Terminal created with 24x80 dimensions");
}

fn test_font_config() {
    use config::ConfigHandle;
    use wezterm_font::FontConfiguration;

    let config = ConfigHandle::default_config();
    println!("  ✓ ConfigHandle::default_config() created");

    let font_config = match FontConfiguration::new(Some(config.clone()), 96) {
        Ok(fc) => {
            println!("  ✓ FontConfiguration::new() succeeded");
            fc
        }
        Err(e) => {
            println!("  ✗ FontConfiguration::new() FAILED: {}", e);
            std::process::exit(1);
        }
    };

    // Try to get the default font metrics - this is what our renderer needs
    match font_config.default_font_metrics() {
        Ok(metrics) => {
            println!("  ✓ default_font_metrics() succeeded:");
            println!("    cell_width: {}", metrics.cell_width.get());
            println!("    cell_height: {}", metrics.cell_height.get());
            println!("    descender: {}", metrics.descender.get());
            println!("    underline_thickness: {}", metrics.underline_thickness.get());
        }
        Err(e) => {
            println!("  ✗ default_font_metrics() FAILED: {}", e);
            println!("    This means the font system cannot find or load any fonts.");
            std::process::exit(1);
        }
    }

    // Try to resolve the default text style font
    let text_style = &config.font;
    match font_config.resolve_font(text_style) {
        Ok(_font) => {
            println!("  ✓ resolve_font() succeeded (default text style)");
        }
        Err(e) => {
            println!("  ✗ resolve_font() FAILED: {}", e);
            std::process::exit(1);
        }
    }
}

fn test_feed_and_read() {
    use wezterm_term::config::TerminalConfiguration;
    use wezterm_term::terminal::Terminal;
    use wezterm_term::TerminalSize;

    #[derive(Debug)]
    struct TestConfig;
    impl TerminalConfiguration for TestConfig {
        fn color_palette(&self) -> wezterm_term::color::ColorPalette {
            wezterm_term::color::ColorPalette::default()
        }
    }

    let size = TerminalSize {
        rows: 24,
        cols: 80,
        pixel_width: 640,
        pixel_height: 384,
        dpi: 96,
    };

    let config: Arc<dyn TerminalConfiguration + Send + Sync> = Arc::new(TestConfig);
    let mut terminal = Terminal::new(
        size,
        config,
        "Agentastic-Test",
        "0.1",
        Box::new(TestWriter),
    );

    // Feed some text
    terminal.advance_bytes(b"Hello, WezTerm!\r\n");
    terminal.advance_bytes(b"Line 2\r\n");

    // Read back the screen
    let screen = terminal.screen_mut();
    let line0 = screen.line_mut(0);
    let text: String = line0.visible_cells().map(|c| c.str().to_string()).collect();
    let trimmed = text.trim_end();
    println!("  Line 0: {:?}", trimmed);
    assert!(trimmed.starts_with("Hello, WezTerm!"));
    println!("  ✓ Terminal received and stored input correctly");

    let cursor = terminal.cursor_pos();
    println!("  Cursor position: ({}, {})", cursor.x, cursor.y);
    assert_eq!(cursor.y, 2);
    println!("  ✓ Cursor advanced to row 2");
}
