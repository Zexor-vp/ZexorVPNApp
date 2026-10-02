//! Счётчик заблокированной рекламы за сессию.
//!
//! xray пишет журнал доступа: по строке на соединение, а у соединений, попавших под правило
//! блокировки, в конце стоит `[socks-in -> block]`. Мы дочитываем файл с того места, где остановились,
//! и считаем такие строки. Недописанную последнюю строку не трогаем — её дочитаем в следующий раз.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Сколько строк в тексте относятся к заблокированным соединениям.
pub fn count_blocked(text: &str) -> u64 {
    text.lines()
        .filter(|line| {
            let line = line.trim_end();
            line.ends_with("-> block]") || line.ends_with(">> block]")
        })
        .count() as u64
}

#[derive(Debug)]
pub struct BlockCounter {
    path: PathBuf,
    offset: u64,
    blocked: u64,
}

impl BlockCounter {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            blocked: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Всего заблокировано с создания счётчика.
    pub fn total(&self) -> u64 {
        self.blocked
    }

    /// Обнуляет счёт: всё, что уже записано в журнал, больше не считается.
    pub fn reset(&mut self) {
        self.blocked = 0;
        self.offset = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
    }

    /// Дочитывает новые строки журнала и возвращает общее число блокировок.
    pub fn poll(&mut self) -> u64 {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return self.blocked;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // Файл пересоздали или обрезали — читаем сначала.
            self.offset = 0;
        }
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return self.blocked;
        }

        let mut chunk = Vec::new();
        if file
            .take(len - self.offset)
            .read_to_end(&mut chunk)
            .is_err()
        {
            return self.blocked;
        }
        // Берём только законченные строки.
        let complete = match chunk.iter().rposition(|b| *b == b'\n') {
            Some(index) => &chunk[..=index],
            None => return self.blocked,
        };
        self.blocked += count_blocked(&String::from_utf8_lossy(complete));
        self.offset += complete.len() as u64;
        self.blocked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const BLOCKED: &str = "2026/10/02 10:00:00 from 127.0.0.1:5000 accepted tcp:ads.example.com:443 [socks-in -> block]";
    const ALLOWED: &str = "2026/10/02 10:00:01 from 127.0.0.1:5001 accepted tcp:site.example.com:443 [socks-in -> proxy]";

    #[test]
    fn counts_only_block_lines() {
        let text = format!("{BLOCKED}\n{ALLOWED}\n{BLOCKED}\n");
        assert_eq!(count_blocked(&text), 2);
        assert_eq!(count_blocked(""), 0);
        assert_eq!(count_blocked("[socks-in -> blocked-by-user]"), 0);
    }

    #[test]
    fn polls_incrementally_and_waits_for_unfinished_lines() {
        let dir = std::env::temp_dir().join(format!("zexor-counter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("access.log");
        let mut counter = BlockCounter::new(&path);
        assert_eq!(counter.poll(), 0); // файла ещё нет

        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{BLOCKED}").unwrap();
        writeln!(file, "{ALLOWED}").unwrap();
        assert_eq!(counter.poll(), 1);

        // Строка записана не до конца — не считаем, пока не появится перевод строки.
        write!(file, "{BLOCKED}").unwrap();
        assert_eq!(counter.poll(), 1);
        writeln!(file).unwrap();
        assert_eq!(counter.poll(), 2);
        assert_eq!(counter.poll(), 2); // повторный опрос ничего не добавляет

        // Файл пересоздали (новая сессия xray) — счётчик продолжает копить, читая заново.
        drop(file);
        std::fs::write(&path, format!("{BLOCKED}\n")).unwrap();
        assert_eq!(counter.poll(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reset_forgets_what_is_already_in_the_log() {
        let dir = std::env::temp_dir().join(format!("zexor-counter-reset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("access.log");
        std::fs::write(&path, format!("{BLOCKED}\n{BLOCKED}\n")).unwrap();

        let mut counter = BlockCounter::new(&path);
        assert_eq!(counter.poll(), 2);
        counter.reset();
        assert_eq!(counter.total(), 0);
        assert_eq!(counter.poll(), 0);

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{BLOCKED}").unwrap();
        assert_eq!(counter.poll(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
