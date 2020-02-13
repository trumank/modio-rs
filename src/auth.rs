//! Authentication Flow interface
use std::error::Error as StdError;
use std::fmt;

use serde::Deserialize;
use url::form_urlencoded;

use crate::routing::Route;
use crate::Modio;
use crate::ModioMessage;
use crate::QueryString;
use crate::Result;

/// [mod.io](https://mod.io) credentials. API key with optional OAuth2 access token.
#[derive(Clone, PartialEq)]
pub struct Credentials {
    pub api_key: String,
    pub token: Option<Token>,
}

/// Access token and optional Unix timestamp of the date this token will expire.
#[derive(Clone, PartialEq)]
pub struct Token {
    pub value: String,
    pub expired_at: Option<u64>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.token.is_some() {
            f.write_str("Credentials(apikey+token)")
        } else {
            f.write_str("Credentials(apikey)")
        }
    }
}

impl Credentials {
    pub fn new<S: Into<String>>(api_key: S) -> Credentials {
        Credentials {
            api_key: api_key.into(),
            token: None,
        }
    }

    pub fn with_token<S: Into<String>, T: Into<String>>(api_key: S, token: T) -> Credentials {
        Credentials {
            api_key: api_key.into(),
            token: Some(Token {
                value: token.into(),
                expired_at: None,
            }),
        }
    }
}

impl From<&str> for Credentials {
    fn from(api_key: &str) -> Credentials {
        Credentials::new(api_key)
    }
}

impl From<(&str, &str)> for Credentials {
    fn from((api_key, token): (&str, &str)) -> Credentials {
        Credentials::with_token(api_key, token)
    }
}

impl From<String> for Credentials {
    fn from(api_key: String) -> Credentials {
        Credentials::new(api_key)
    }
}

impl From<(String, String)> for Credentials {
    fn from((api_key, token): (String, String)) -> Credentials {
        Credentials::with_token(api_key, token)
    }
}

/// Authentication error
#[derive(Debug)]
pub enum Error {
    /// API key/access token is incorrect, revoked or expired.
    Unauthorized,
    /// Access token is required to perform the action.
    TokenRequired,
}

impl StdError for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unauthorized => f.write_str("Unauthorized"),
            Error::TokenRequired => f.write_str("Access token is required"),
        }
    }
}

/// Authentication Flow interface to retrieve access tokens. See the [mod.io Authentication
/// docs](https://docs.mod.io/#email-authentication-flow) for more information.
///
/// # Example
/// ```no_run
/// use std::io::{self, Write};
///
/// use modio::{Credentials, Modio, Result};
///
/// fn prompt(prompt: &str) -> io::Result<String> {
///     print!("{}", prompt);
///     io::stdout().flush()?;
///     let mut buffer = String::new();
///     io::stdin().read_line(&mut buffer)?;
///     Ok(buffer.trim().to_string())
/// }
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     let modio = Modio::new(Credentials::new("api-key"))?;
///
///     let email = prompt("Enter email: ").expect("read email");
///     modio.auth().request_code(&email).await?;
///
///     let code = prompt("Enter security code: ").expect("read code");
///     let token = modio.auth().security_code(&code).await?;
///
///     // Consume the endpoint and create an endpoint with new credentials.
///     let _modio = modio.with_credentials(token);
///
///     Ok(())
/// }
/// ```
pub struct Auth {
    modio: Modio,
}

#[derive(Deserialize)]
struct AccessToken {
    #[serde(rename = "access_token")]
    value: String,
    #[serde(rename = "date_expires")]
    expired_at: Option<u64>,
}

impl Auth {
    pub(crate) fn new(modio: Modio) -> Self {
        Self { modio }
    }

    /// Request a security code be sent to the email of the user. [required: apikey]
    pub async fn request_code(self, email: &str) -> Result<()> {
        let data = form_urlencoded::Serializer::new(String::new())
            .append_pair("email", email)
            .finish();

        self.modio
            .request(Route::AuthEmailRequest)
            .body(data)
            .send::<ModioMessage>()
            .await?;

        Ok(())
    }

    /// Get the access token for a security code. [required: apikey]
    pub async fn security_code(self, code: &str) -> Result<Credentials> {
        let data = form_urlencoded::Serializer::new(String::new())
            .append_pair("security_code", code)
            .finish();

        let t = self
            .modio
            .request(Route::AuthEmailExchange)
            .body(data)
            .send::<AccessToken>()
            .await?;

        let token = Token {
            value: t.value,
            expired_at: t.expired_at,
        };
        Ok(Credentials {
            api_key: self.modio.credentials.api_key,
            token: Some(token),
        })
    }

    /// Authenticate via external services ([Steam], [GOG], [itch.io], [Oculus]).
    ///
    /// See the [mod.io docs](https://docs.mod.io/#authentication-2) for more information.
    ///
    /// [Steam]: struct.SteamOptions.html
    /// [GOG]: struct.GalaxyOptions.html
    /// [itch.io]: struct.ItchioOptions.html
    /// [Oculus]: struct.OculusOptions.html
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use modio::{Credentials, Modio, Result};
    /// # #[tokio::main]
    /// # async fn run() -> Result<()> {
    /// #   let modio = modio::Modio::new("apikey")?;
    /// use modio::auth::SteamOptions;
    /// let opts = SteamOptions::new("ticket");
    /// modio.auth().external(opts).await?;
    ///
    /// use modio::auth::GalaxyOptions;
    /// let opts = GalaxyOptions::new("ticket").email("foobar@example.com");
    /// modio.auth().external(opts).await?;
    ///
    /// use modio::auth::ItchioOptions;
    /// # let now = 1;
    /// # let two_weeks = 2;
    /// let opts = ItchioOptions::new("token").expired_at(now + two_weeks);
    /// modio.auth().external(opts).await?;
    /// #   Ok(())
    /// # }
    /// ```
    pub async fn external<T>(self, auth_options: T) -> Result<Credentials>
    where
        T: Into<AuthOptions>,
    {
        let (route, data) = match auth_options.into() {
            AuthOptions::Gog(opts) => (Route::AuthGog, opts.to_query_string()),
            AuthOptions::Itchio(opts) => (Route::AuthItchio, opts.to_query_string()),
            AuthOptions::Oculus(opts) => (Route::AuthOculus, opts.to_query_string()),
            AuthOptions::Steam(opts) => (Route::AuthSteam, opts.to_query_string()),
        };

        let t = self
            .modio
            .request(route)
            .body(data)
            .send::<AccessToken>()
            .await?;

        let token = Token {
            value: t.value,
            expired_at: t.expired_at,
        };
        Ok(Credentials {
            api_key: self.modio.credentials.api_key,
            token: Some(token),
        })
    }

    /// Link an external account. Requires an auth token from the external platform.
    ///
    /// See the [mod.io docs](https://docs.mod.io/#link-external-account) for more information.
    pub async fn link(self, options: LinkOptions) -> Result<()> {
        self.modio
            .request(Route::LinkAccount)
            .body(options.to_query_string())
            .send::<ModioMessage>()
            .await?;

        Ok(())
    }
}

/// Various options for external authentication.
pub enum AuthOptions {
    Gog(GalaxyOptions),
    Itchio(ItchioOptions),
    Oculus(OculusOptions),
    Steam(SteamOptions),
}

// impl From<*Options> for AuthOptions {{{
impl From<GalaxyOptions> for AuthOptions {
    fn from(options: GalaxyOptions) -> AuthOptions {
        AuthOptions::Gog(options)
    }
}

impl From<ItchioOptions> for AuthOptions {
    fn from(options: ItchioOptions) -> AuthOptions {
        AuthOptions::Itchio(options)
    }
}

impl From<OculusOptions> for AuthOptions {
    fn from(options: OculusOptions) -> AuthOptions {
        AuthOptions::Oculus(options)
    }
}

impl From<SteamOptions> for AuthOptions {
    fn from(options: SteamOptions) -> AuthOptions {
        AuthOptions::Steam(options)
    }
}
// }}}

/// Authentication options for an encrypted gog app ticket.
///
/// See the [mod.io docs](https://docs.mod.io/#authenticate-via-gog-galaxy) for more information.
pub struct GalaxyOptions {
    params: std::collections::BTreeMap<&'static str, String>,
}

impl GalaxyOptions {
    pub fn new<T>(ticket: T) -> Self
    where
        T: Into<String>,
    {
        let mut params = std::collections::BTreeMap::new();
        params.insert("appdata", ticket.into());
        Self { params }
    }

    option!(email >> "email");
    option!(
        /// Unix timestamp of date in which the returned token will expire. Value cannot be higher
        /// than the default value which is a common year.
        expired_at: u64 >> "date_expires"
    );
}

impl QueryString for GalaxyOptions {
    fn to_query_string(&self) -> String {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&self.params)
            .finish()
    }
}

/// Authentication options for an itch.io JWT token.
///
/// See the [mod.io docs](https://docs.mod.io/#authenticate-via-itch-io) for more information.
pub struct ItchioOptions {
    params: std::collections::BTreeMap<&'static str, String>,
}

impl ItchioOptions {
    pub fn new<T>(token: T) -> Self
    where
        T: Into<String>,
    {
        let mut params = std::collections::BTreeMap::new();
        params.insert("itchio_token", token.into());
        Self { params }
    }

    option!(email >> "email");
    option!(
        /// Unix timestamp of date in which the returned token will expire. Value cannot be higher
        /// than the default value which is a week.
        expired_at: u64 >> "date_expires"
    );
}

impl QueryString for ItchioOptions {
    fn to_query_string(&self) -> String {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&self.params)
            .finish()
    }
}

/// Authentication options for an Oculus user.
///
/// See the [mod.io docs](https://docs.mod.io/#authenticate-via-oculus) for more information.
pub struct OculusOptions {
    params: std::collections::BTreeMap<&'static str, String>,
}

impl OculusOptions {
    pub fn new<T>(nonce: T, user_id: u64, auth_token: T) -> Self
    where
        T: Into<String>,
    {
        let mut params = std::collections::BTreeMap::new();
        params.insert("nonce", nonce.into());
        params.insert("user_id", user_id.to_string());
        params.insert("auth_token", auth_token.into());
        Self { params }
    }

    option!(email >> "email");
    option!(
        /// Unix timestamp of date in which the returned token will expire. Value cannot be higher
        /// than the default value which is a common year.
        expired_at: u64 >> "date_expires"
    );
}

impl QueryString for OculusOptions {
    fn to_query_string(&self) -> String {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&self.params)
            .finish()
    }
}

/// Authentication options for an encrypted steam app ticket.
///
/// See the [mod.io docs](https://docs.mod.io/#authenticate-via-steam) for more information.
pub struct SteamOptions {
    params: std::collections::BTreeMap<&'static str, String>,
}

impl SteamOptions {
    pub fn new<T>(ticket: T) -> Self
    where
        T: Into<String>,
    {
        let mut params = std::collections::BTreeMap::new();
        params.insert("appdata", ticket.into());
        Self { params }
    }

    option!(email >> "email");
    option!(
        /// Unix timestamp of date in which the returned token will expire. Value cannot be higher
        /// than the default value which is a common year.
        expired_at: u64 >> "date_expires"
    );
}

impl QueryString for SteamOptions {
    fn to_query_string(&self) -> String {
        form_urlencoded::Serializer::new(String::new())
            .extend_pairs(&self.params)
            .finish()
    }
}

/// Options for connecting external accounts with the authenticated user's email address.
pub struct LinkOptions {
    email: String,
    service: Service,
}

impl LinkOptions {
    pub fn steam<S: Into<String>>(email: S, steam_id: u64) -> Self {
        Self {
            email: email.into(),
            service: Service::Steam(steam_id),
        }
    }

    pub fn gog<S: Into<String>>(email: S, gog_id: u64) -> Self {
        Self {
            email: email.into(),
            service: Service::Gog(gog_id),
        }
    }

    pub fn itchio<S: Into<String>>(email: S, itchio_id: u64) -> Self {
        Self {
            email: email.into(),
            service: Service::Itchio(itchio_id),
        }
    }
}

impl QueryString for LinkOptions {
    fn to_query_string(&self) -> String {
        let (service, id) = match self.service {
            Service::Steam(id) => ("steam", id.to_string()),
            Service::Gog(id) => ("gog", id.to_string()),
            Service::Itchio(id) => ("itch", id.to_string()),
        };
        form_urlencoded::Serializer::new(String::new())
            .append_pair("email", &self.email)
            .append_pair("service", service)
            .append_pair("service_id", &id)
            .finish()
    }
}

enum Service {
    Steam(u64),
    Gog(u64),
    Itchio(u64),
}

// vim: fdm=marker
