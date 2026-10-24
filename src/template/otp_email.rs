//! Email kode OTP — 6 kotak angka besar, masa berlaku, peringatan jangan dibagikan, dan catatan
//! "tidak merasa meminta". Dipakai untuk apa pun yang butuh kode sekali pakai: cukup isi kata-katanya.
//!
//! ```ignore
//! let email = OtpEmail {
//!     name: &user.full_name,
//!     code: &otp,
//!     valid_minutes: 10,
//!     label: "Verifikasi login",
//!     subject: "kode login Anda",
//!     message: "Masukkan kode berikut untuk menyelesaikan login.",
//!     ignore_note: "Tidak merasa login? Segera ganti password Anda.",
//! }
//! .render();
//! send_email(&user.email, &email.subject, email.text, email.html).await?;
//! ```

use super::{escape_html, RenderedEmail, APP_NAME, LOGO_CID};

pub struct OtpEmail<'a> {
    /// Nama penerima, untuk sapaan "Halo {name},".
    pub name: &'a str,
    /// Kode OTP (biasanya 6 digit) — tiap karakter jadi 1 kotak.
    pub code: &'a str,
    /// Masa berlaku kode, ditampilkan "Berlaku N menit".
    pub valid_minutes: i64,
    /// Label kecil di atas sapaan, mis. "Reset password" / "Verifikasi login".
    pub label: &'a str,
    /// Lanjutan subjek: "{code} adalah {subject}", mis. "kode reset password Anda" — kode langsung
    /// terlihat di notifikasi HP.
    pub subject: &'a str,
    /// Paragraf penjelasan di bawah sapaan.
    pub message: &'a str,
    /// Catatan untuk penerima yang tidak merasa meminta kode.
    pub ignore_note: &'a str,
}

impl OtpEmail<'_> {
    pub fn render(&self) -> RenderedEmail {
        let subject = format!("{} adalah {}", self.code, self.subject);
        RenderedEmail {
            text: self.render_text(),
            html: self.render_html(&subject),
            subject,
        }
    }

    fn render_text(&self) -> String {
        format!(
            "Halo {name},\n\n\
             {message}\n\n\
             Kode OTP Anda: {code}\n\n\
             Kode ini berlaku {minutes} menit dan hanya bisa dipakai sekali.\n\
             Jangan berikan kode ini kepada siapa pun, termasuk yang mengaku dari tim kami.\n\n\
             {ignore}\n\n\
             — {APP_NAME}",
            name = self.name,
            message = self.message,
            code = self.code,
            minutes = self.valid_minutes,
            ignore = self.ignore_note,
        )
    }

    fn render_html(&self, subject: &str) -> String {
        let digits: String = self
            .code
            .chars()
            .map(|digit| {
                format!(
                    r#"<td style="padding:0 4px;"><div style="width:44px;height:56px;line-height:56px;border-radius:10px;background:#f1f5f9;border:1px solid #e2e8f0;font-family:'SF Mono',Menlo,Consolas,monospace;font-size:28px;font-weight:700;color:#0f172a;text-align:center;">{}</div></td>"#,
                    escape_html(&digit.to_string())
                )
            })
            .collect();

        let subject = escape_html(subject);
        let code = escape_html(self.code);
        let name = escape_html(self.name);
        let label = escape_html(self.label);
        let message = escape_html(self.message);
        let ignore_note = escape_html(self.ignore_note);
        let minutes = self.valid_minutes;

        format!(
            r#"<!doctype html>
<html lang="id">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light">
<title>{subject}</title>
</head>
<body style="margin:0;padding:0;background:#f1f5f9;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;">{label}: kode Anda {code}, berlaku {minutes} menit.</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f1f5f9;padding:32px 12px;">
  <tr><td align="center">
    <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:480px;background:#ffffff;border-radius:16px;overflow:hidden;box-shadow:0 4px 24px rgba(15,23,42,0.08);font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">
      <tr>
        <td style="height:4px;line-height:4px;font-size:0;background:linear-gradient(90deg,#22d3ee 0%,#3b82f6 50%,#8b5cf6 100%);background-color:#3b82f6;">&nbsp;</td>
      </tr>
      <tr>
        <td align="center" style="padding:24px 32px 0;background:#ffffff;">
          <img src="cid:{LOGO_CID}" width="96" height="96" alt="{APP_NAME}" style="display:block;width:96px;height:96px;border:0;outline:none;text-decoration:none;">
        </td>
      </tr>
      <tr>
        <td style="padding:20px 32px 8px;">
          <p style="margin:0 0 6px;font-size:13px;font-weight:600;letter-spacing:0.8px;text-transform:uppercase;color:#64748b;">{label}</p>
          <h1 style="margin:0 0 16px;font-size:22px;line-height:1.3;color:#0f172a;">Halo {name},</h1>
          <p style="margin:0;font-size:15px;line-height:1.6;color:#334155;">{message}</p>
        </td>
      </tr>
      <tr>
        <td align="center" style="padding:24px 24px 8px;">
          <table role="presentation" cellpadding="0" cellspacing="0"><tr>{digits}</tr></table>
        </td>
      </tr>
      <tr>
        <td align="center" style="padding:8px 32px 28px;">
          <p style="margin:0;font-size:13px;color:#64748b;">Berlaku <strong style="color:#0f172a;">{minutes} menit</strong> &middot; hanya bisa dipakai sekali</p>
        </td>
      </tr>
      <tr>
        <td style="padding:0 32px 28px;">
          <table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#fff7ed;border:1px solid #fed7aa;border-radius:10px;">
            <tr><td style="padding:14px 16px;font-size:13px;line-height:1.55;color:#9a3412;">
              <strong>Jaga kerahasiaan kode ini.</strong> Jangan berikan kepada siapa pun, termasuk orang yang mengaku dari tim kami.
            </td></tr>
          </table>
        </td>
      </tr>
      <tr>
        <td style="padding:0 32px 32px;font-size:13px;line-height:1.6;color:#64748b;">{ignore_note}</td>
      </tr>
      <tr>
        <td style="padding:18px 32px;background:#f8fafc;border-top:1px solid #e2e8f0;font-size:12px;color:#94a3b8;text-align:center;">
          Email ini dikirim otomatis, mohon tidak membalas.<br>&copy; {APP_NAME}
        </td>
      </tr>
    </table>
  </td></tr>
</table>
</body>
</html>"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample<'a>(name: &'a str, message: &'a str) -> OtpEmail<'a> {
        OtpEmail {
            name,
            code: "048213",
            valid_minutes: 10,
            label: "Verifikasi login",
            subject: "kode login Anda",
            message,
            ignore_note: "Tidak merasa login? Segera ganti password Anda.",
        }
    }

    #[test]
    fn kata_kata_dari_parameter() {
        let email = sample("Budi", "Masukkan kode berikut untuk login.").render();
        assert_eq!(email.subject, "048213 adalah kode login Anda");
        assert!(email.text.contains("Masukkan kode berikut untuk login.") && email.text.contains("10 menit"));
        assert!(email.html.contains("Verifikasi login") && email.html.contains("Segera ganti password"));
        // 6 kotak digit.
        assert_eq!(email.html.matches("border-radius:10px;background:#f1f5f9").count(), 6);
    }

    #[test]
    fn teks_parameter_di_escape() {
        let email = sample("Budi <script>", "<b>halo</b>").render();
        assert!(email.html.contains("Budi &lt;script&gt;") && email.html.contains("&lt;b&gt;halo&lt;/b&gt;"));
        assert!(!email.html.contains("<script>") && !email.html.contains("<b>halo"));
    }
}
