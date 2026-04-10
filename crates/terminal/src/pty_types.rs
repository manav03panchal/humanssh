pub(crate) const PROCESS_NAME_MAX_LEN: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InlineProcessName {
    buf: [u8; PROCESS_NAME_MAX_LEN],
    len: u8,
}

impl std::fmt::Display for InlineProcessName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl InlineProcessName {
    pub(crate) fn new(s: &str) -> Self {
        let truncated = if s.len() <= PROCESS_NAME_MAX_LEN {
            s
        } else {
            let mut end = PROCESS_NAME_MAX_LEN;
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            &s[..end]
        };
        let bytes = truncated.as_bytes();
        let mut buf = [0u8; PROCESS_NAME_MAX_LEN];
        buf[..bytes.len()].copy_from_slice(bytes);
        Self {
            buf,
            len: bytes.len() as u8,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        // SAFETY: new() guarantees we store valid UTF-8 at a char boundary
        debug_assert!(std::str::from_utf8(&self.buf[..self.len as usize]).is_ok());
        unsafe { std::str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
    }
}

#[derive(Default)]
pub(crate) struct ProcessCache {
    pub(crate) has_children: bool,
    pub(crate) process_name: Option<InlineProcessName>,
    pub(crate) last_update: Option<std::time::Instant>,
}

pub(crate) const PROCESS_CACHE_TTL_MS: u64 = 500;
