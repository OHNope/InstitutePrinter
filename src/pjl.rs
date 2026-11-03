// PJL wrapper around the print data. The job start, language switch and job
// end strings are the ones in Kyocera's PPD for this printer (*JCLBegin,
// *JCLToPSInterpreter, *JCLEnd). The PPD sets duplex and color mode with
// PostScript code, which PDF direct printing does not run, so those settings
// use the standard HP PJL variables instead.

const JOB_BEGIN: &[u8] = b"\x1b%-12345X@PJL JOB\n";
pub const JOB_END: &[u8] = b"\x1b%-12345X@PJL EOJ\n\x1b%-12345X";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Pdf,
    PostScript,
}

impl Language {
    pub fn detect(head: &[u8]) -> Option<Language> {
        if head.starts_with(b"%PDF-") {
            Some(Language::Pdf)
        } else if head.starts_with(b"%!") {
            Some(Language::PostScript)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::Pdf => "PDF",
            Language::PostScript => "PostScript",
        }
    }

    fn pjl_name(self) -> &'static str {
        match self {
            Language::Pdf => "PDF",
            Language::PostScript => "POSTSCRIPT",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duplex {
    Off,
    LongEdge,
    ShortEdge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Color,
    Mono,
}

// Settings left as None are not sent, so the printer's own defaults apply.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub duplex: Option<Duplex>,
    pub color: Option<ColorMode>,
}

pub fn header(language: Language, settings: &Settings) -> Vec<u8> {
    let mut out = JOB_BEGIN.to_vec();

    match settings.duplex {
        Some(Duplex::Off) => out.extend_from_slice(b"@PJL SET DUPLEX=OFF\n"),
        Some(Duplex::LongEdge) => {
            out.extend_from_slice(b"@PJL SET DUPLEX=ON\n@PJL SET BINDING=LONGEDGE\n")
        }
        Some(Duplex::ShortEdge) => {
            out.extend_from_slice(b"@PJL SET DUPLEX=ON\n@PJL SET BINDING=SHORTEDGE\n")
        }
        None => {}
    }

    match settings.color {
        Some(ColorMode::Color) => out.extend_from_slice(b"@PJL SET RENDERMODE=COLOR\n"),
        Some(ColorMode::Mono) => out.extend_from_slice(b"@PJL SET RENDERMODE=GRAYSCALE\n"),
        None => {}
    }

    out.extend_from_slice(format!("@PJL ENTER LANGUAGE={}\n", language.pjl_name()).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_recognizes_pdf_and_postscript() {
        assert_eq!(Language::detect(b"%PDF-1.7\n"), Some(Language::Pdf));
        assert_eq!(
            Language::detect(b"%!PS-Adobe-3.0\n"),
            Some(Language::PostScript)
        );
        assert_eq!(Language::detect(b"PK\x03\x04"), None);
        assert_eq!(Language::detect(b""), None);
    }

    #[test]
    fn header_without_settings_only_switches_language() {
        assert_eq!(
            header(Language::Pdf, &Settings::default()),
            b"\x1b%-12345X@PJL JOB\n@PJL ENTER LANGUAGE=PDF\n".to_vec()
        );
    }

    #[test]
    fn header_sets_duplex_and_color() {
        let settings = Settings {
            duplex: Some(Duplex::ShortEdge),
            color: Some(ColorMode::Mono),
        };
        assert_eq!(
            String::from_utf8(header(Language::PostScript, &settings)).unwrap(),
            "\x1b%-12345X@PJL JOB\n\
             @PJL SET DUPLEX=ON\n\
             @PJL SET BINDING=SHORTEDGE\n\
             @PJL SET RENDERMODE=GRAYSCALE\n\
             @PJL ENTER LANGUAGE=POSTSCRIPT\n"
        );
    }

    #[test]
    fn header_can_turn_duplex_off() {
        let settings = Settings {
            duplex: Some(Duplex::Off),
            color: Some(ColorMode::Color),
        };
        let text = String::from_utf8(header(Language::Pdf, &settings)).unwrap();
        assert!(text.contains("@PJL SET DUPLEX=OFF\n"));
        assert!(!text.contains("BINDING"));
        assert!(text.contains("@PJL SET RENDERMODE=COLOR\n"));
    }
}
