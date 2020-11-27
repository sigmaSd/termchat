use once_cell::sync::Lazy;
use termchat::application::{Application, Event, Config};
use message_io::events::EventSender;
use std::io::stdout;
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use std::io::Write;

static CONFIG: Lazy<Config> = Lazy::new(|| Config {
    discovery_addr: "238.255.0.1:5877".parse().unwrap(),
    tcp_server_port: "0".parse().unwrap(),
    user_name: "A".to_string(),
});
static CONFIG2: Lazy<Config> = Lazy::new(|| Config {
    discovery_addr: "238.255.0.1:5877".parse().unwrap(),
    tcp_server_port: "0".parse().unwrap(),
    user_name: "B".to_string(),
});

#[test]
fn send_file() {
    let termchat_dir = std::env::temp_dir().join("termchat");
    let test_path = termchat_dir.join("test");
    let _ = std::fs::remove_dir_all(&termchat_dir);
    std::fs::create_dir_all(&termchat_dir).unwrap();

    let data = vec![rand::random(); 10usize.pow(6)];
    std::fs::write(&test_path, &data).unwrap();

    let (mut s1, t1) = test_user(1);
    let (s2, t2) = test_user(2);

    // wait for users to connect
    std::thread::sleep(std::time::Duration::from_millis(100));
    // send file
    input(&mut s1, &format!("?send {}", test_path.display()));
    // wait for the file to finish sending
    std::thread::sleep(std::time::Duration::from_secs(2));

    // finish
    s1.send(Event::Close(None));
    s2.send(Event::Close(None));
    t1.join().unwrap();
    t2.join().unwrap();

    // assert eq
    let send_data =
        std::fs::read(std::env::temp_dir().join("termchat").join("A").join("test")).unwrap();
    assert_eq!(data.len(), send_data.len());
    assert_eq!(data, send_data);
}

fn test_user(n: usize) -> (EventSender<Event>, std::thread::JoinHandle<()>) {
    let config = if n == 1 { &CONFIG } else { &CONFIG2 };
    let mut app = Application::new(config).unwrap();
    let sender = app.event_queue.sender().clone();
    let t = std::thread::spawn(move || {
        let mut f =
            std::fs::File::create(std::env::temp_dir().join("termchat").join(n.to_string()))
                .unwrap();
        app.run(&mut f).unwrap();
    });
    (sender, t)
}

fn input(sender: &mut EventSender<Event>, s: &str) {
    for c in s.chars() {
        sender.send(Event::Terminal(crossterm::event::Event::Key(crossterm::event::KeyEvent {
            code: crossterm::event::KeyCode::Char(c),
            modifiers: crossterm::event::KeyModifiers::NONE,
        })));
    }
    sender.send(Event::Terminal(crossterm::event::Event::Key(crossterm::event::KeyEvent {
        code: crossterm::event::KeyCode::Enter,
        modifiers: crossterm::event::KeyModifiers::NONE,
    })));
}
