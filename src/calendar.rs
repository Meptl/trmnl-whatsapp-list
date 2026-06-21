use std::collections::HashSet;
use std::error::Error;
use std::fmt;

use jiff::{Timestamp, civil::Date};
use serde::{Deserialize, Serialize};

use crate::config::{GoogleCalendarConfig, SecretString};

const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const CALENDAR_LIST_URL: &str = "https://www.googleapis.com/calendar/v3/users/me/calendarList";
const CALENDAR_API_BASE_URL: &str = "https://www.googleapis.com/calendar/v3";

#[derive(Clone)]
pub struct GoogleCalendarClient {
    http_client: reqwest::Client,
    client_id: String,
    client_secret: SecretString,
    refresh_token: SecretString,
    #[cfg(test)]
    test_events: Option<TestCalendarEvents>,
}

#[cfg(test)]
#[derive(Clone)]
enum TestCalendarEvents {
    Available(CalendarDisplayDay),
    Unavailable,
}

impl GoogleCalendarClient {
    pub fn new(config: &GoogleCalendarConfig) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
            refresh_token: config.refresh_token.clone(),
            #[cfg(test)]
            test_events: None,
        }
    }

    #[cfg(test)]
    pub fn test_with_events(events: Vec<CalendarDisplayEvent>) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            client_id: "google-client-id".to_owned(),
            client_secret: SecretString::from_test_value("google-client-secret"),
            refresh_token: SecretString::from_test_value("google-refresh-token"),
            test_events: Some(TestCalendarEvents::Available(
                CalendarDisplayDay::for_tests(events),
            )),
        }
    }

    #[cfg(test)]
    pub fn test_unavailable() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            client_id: "google-client-id".to_owned(),
            client_secret: SecretString::from_test_value("google-client-secret"),
            refresh_token: SecretString::from_test_value("google-refresh-token"),
            test_events: Some(TestCalendarEvents::Unavailable),
        }
    }

    pub async fn today_events(&self) -> Result<CalendarDisplayDay, CalendarError> {
        #[cfg(test)]
        if let Some(test_events) = &self.test_events {
            return match test_events {
                TestCalendarEvents::Available(day) => Ok(day.clone()),
                TestCalendarEvents::Unavailable => Err(CalendarError::TestUnavailable),
            };
        }

        let access_token = self.refresh_access_token().await?;
        let calendars = self.fetch_calendar_list(&access_token).await?;
        let primary_time_zone = primary_calendar_time_zone(&calendars)?;
        let window = CalendarDayWindow::for_today(&primary_time_zone)?;
        let selected_calendars = selected_visible_calendars(&calendars);
        let mut events = Vec::new();

        for calendar in selected_calendars {
            events.extend(
                self.fetch_events(&access_token, &calendar.id, &window)
                    .await?,
            );
        }

        let events = normalize_events(events, &window)?;

        Ok(CalendarDisplayDay::new(
            calendar_date_label(window.today),
            events,
        ))
    }

    pub fn build_refresh_token_request(&self) -> Result<reqwest::Request, CalendarError> {
        self.http_client
            .post(OAUTH_TOKEN_URL)
            .form(&RefreshTokenRequest {
                client_id: &self.client_id,
                client_secret: self.client_secret.as_str(),
                refresh_token: self.refresh_token.as_str(),
                grant_type: "refresh_token",
            })
            .build()
            .map_err(CalendarError::RequestBuild)
    }

    async fn refresh_access_token(&self) -> Result<String, CalendarError> {
        let request = self.build_refresh_token_request()?;
        let response = self
            .http_client
            .execute(request)
            .await
            .map_err(CalendarError::Send)?;
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.map_err(CalendarError::Send)?;
            return Err(CalendarError::HttpStatus {
                operation: "refresh Google OAuth token",
                status,
                body,
            });
        }

        let token = response
            .json::<RefreshTokenResponse>()
            .await
            .map_err(CalendarError::Decode)?;
        Ok(token.access_token)
    }

    async fn fetch_calendar_list(
        &self,
        access_token: &str,
    ) -> Result<Vec<CalendarListEntry>, CalendarError> {
        let mut calendars = Vec::new();
        let mut page_token = None;

        loop {
            let request = self.build_calendar_list_request(access_token, page_token.as_deref())?;
            let response = self
                .execute_json::<CalendarListResponse>(request, "list Google calendars")
                .await?;
            calendars.extend(response.items.unwrap_or_default());

            match response.next_page_token {
                Some(next_page_token) => page_token = Some(next_page_token),
                None => return Ok(calendars),
            }
        }
    }

    fn build_calendar_list_request(
        &self,
        access_token: &str,
        page_token: Option<&str>,
    ) -> Result<reqwest::Request, CalendarError> {
        let mut request = self
            .http_client
            .get(CALENDAR_LIST_URL)
            .bearer_auth(access_token);
        if let Some(page_token) = page_token {
            request = request.query(&[("pageToken", page_token)]);
        }

        request.build().map_err(CalendarError::RequestBuild)
    }

    async fn fetch_events(
        &self,
        access_token: &str,
        calendar_id: &str,
        window: &CalendarDayWindow,
    ) -> Result<Vec<GoogleEvent>, CalendarError> {
        let mut events = Vec::new();
        let mut page_token = None;

        loop {
            let request = self.build_events_request(
                access_token,
                calendar_id,
                window,
                page_token.as_deref(),
            )?;
            let response = self
                .execute_json::<EventsListResponse>(request, "list Google calendar events")
                .await?;
            events.extend(response.items.unwrap_or_default());

            match response.next_page_token {
                Some(next_page_token) => page_token = Some(next_page_token),
                None => return Ok(events),
            }
        }
    }

    fn build_events_request(
        &self,
        access_token: &str,
        calendar_id: &str,
        window: &CalendarDayWindow,
        page_token: Option<&str>,
    ) -> Result<reqwest::Request, CalendarError> {
        let mut url = reqwest::Url::parse(CALENDAR_API_BASE_URL)
            .map_err(|error| CalendarError::Url(error.to_string()))?;
        url.path_segments_mut()
            .map_err(|_| CalendarError::Url("calendar API base URL cannot be a base".to_owned()))?
            .push("calendars")
            .push(calendar_id)
            .push("events");

        let mut query = vec![
            ("timeMin", window.time_min.as_str()),
            ("timeMax", window.time_max.as_str()),
            ("timeZone", window.time_zone.as_str()),
            ("singleEvents", "true"),
            ("orderBy", "startTime"),
        ];
        if let Some(page_token) = page_token {
            query.push(("pageToken", page_token));
        }

        self.http_client
            .get(url)
            .bearer_auth(access_token)
            .query(&query)
            .build()
            .map_err(CalendarError::RequestBuild)
    }

    async fn execute_json<T: for<'de> Deserialize<'de>>(
        &self,
        request: reqwest::Request,
        operation: &'static str,
    ) -> Result<T, CalendarError> {
        let response = self
            .http_client
            .execute(request)
            .await
            .map_err(CalendarError::Send)?;
        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.map_err(CalendarError::Send)?;
            return Err(CalendarError::HttpStatus {
                operation,
                status,
                body,
            });
        }

        response.json::<T>().await.map_err(CalendarError::Decode)
    }
}

impl fmt::Debug for GoogleCalendarClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GoogleCalendarClient")
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum CalendarError {
    RequestBuild(reqwest::Error),
    Send(reqwest::Error),
    Decode(reqwest::Error),
    HttpStatus {
        operation: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },
    MissingPrimaryCalendarTimeZone,
    InvalidTime(jiff::Error),
    InvalidCalendarData(&'static str),
    Url(String),
    #[cfg(test)]
    TestUnavailable,
}

impl fmt::Display for CalendarError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestBuild(error) => write!(
                formatter,
                "failed to build Google Calendar request: {error}"
            ),
            Self::Send(error) => {
                write!(formatter, "failed to send Google Calendar request: {error}")
            }
            Self::Decode(error) => write!(
                formatter,
                "failed to decode Google Calendar response: {error}"
            ),
            Self::HttpStatus {
                operation,
                status,
                body,
            } => write!(
                formatter,
                "Google Calendar operation `{operation}` returned HTTP {status}: {body}"
            ),
            Self::MissingPrimaryCalendarTimeZone => write!(
                formatter,
                "Google Calendar primary calendar time zone was not available"
            ),
            Self::InvalidTime(error) => {
                write!(formatter, "invalid Google Calendar time value: {error}")
            }
            Self::InvalidCalendarData(reason) => {
                write!(formatter, "invalid Google Calendar data: {reason}")
            }
            Self::Url(error) => write!(formatter, "invalid Google Calendar URL: {error}"),
            #[cfg(test)]
            Self::TestUnavailable => write!(formatter, "test Google Calendar data unavailable"),
        }
    }
}

impl Error for CalendarError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::RequestBuild(error) | Self::Send(error) | Self::Decode(error) => Some(error),
            Self::InvalidTime(error) => Some(error),
            Self::HttpStatus { .. }
            | Self::MissingPrimaryCalendarTimeZone
            | Self::InvalidCalendarData(_)
            | Self::Url(_) => None,
            #[cfg(test)]
            Self::TestUnavailable => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CalendarDisplayDay {
    date_label: String,
    events: Vec<CalendarDisplayEvent>,
}

impl CalendarDisplayDay {
    fn new(date_label: impl Into<String>, events: Vec<CalendarDisplayEvent>) -> Self {
        Self {
            date_label: date_label.into(),
            events,
        }
    }

    pub fn date_label(&self) -> &str {
        &self.date_label
    }

    pub fn events(&self) -> &[CalendarDisplayEvent] {
        &self.events
    }

    #[cfg(test)]
    fn for_tests(events: Vec<CalendarDisplayEvent>) -> Self {
        Self::new("Jun 21", events)
    }
}

pub(crate) fn calendar_date_label(date: Date) -> String {
    date.strftime("%b %-d").to_string()
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CalendarDisplayEvent {
    prefix: String,
    title: String,
    sort_group: u8,
    sort_seconds: i64,
    dedupe_key: String,
}

impl CalendarDisplayEvent {
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn hash_fields(&self) -> impl Iterator<Item = &str> {
        [
            self.prefix.as_str(),
            self.title.as_str(),
            self.dedupe_key.as_str(),
        ]
        .into_iter()
    }

    #[cfg(test)]
    pub fn test_event(prefix: impl Into<String>, title: impl Into<String>) -> Self {
        let prefix = prefix.into();
        let title = title.into();
        Self {
            dedupe_key: format!("{prefix}:{title}"),
            prefix,
            title,
            sort_group: 1,
            sort_seconds: 0,
        }
    }
}

#[derive(Serialize)]
struct RefreshTokenRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    refresh_token: &'a str,
    grant_type: &'a str,
}

#[derive(Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct CalendarListResponse {
    items: Option<Vec<CalendarListEntry>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CalendarListEntry {
    id: Option<String>,
    #[serde(rename = "timeZone")]
    time_zone: Option<String>,
    selected: Option<bool>,
    hidden: Option<bool>,
    primary: Option<bool>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct CalendarRef {
    id: String,
}

fn primary_calendar_time_zone(calendars: &[CalendarListEntry]) -> Result<String, CalendarError> {
    calendars
        .iter()
        .find(|calendar| calendar.primary.unwrap_or(false))
        .and_then(|calendar| calendar.time_zone.clone())
        .ok_or(CalendarError::MissingPrimaryCalendarTimeZone)
}

fn selected_visible_calendars(calendars: &[CalendarListEntry]) -> Vec<CalendarRef> {
    calendars
        .iter()
        .filter(|calendar| calendar.selected.unwrap_or(false))
        .filter(|calendar| !calendar.hidden.unwrap_or(false))
        .filter_map(|calendar| calendar.id.clone().map(|id| CalendarRef { id }))
        .collect()
}

#[derive(Debug, Clone)]
struct CalendarDayWindow {
    time_zone: String,
    today: Date,
    start_timestamp: Timestamp,
    end_timestamp: Timestamp,
    time_min: String,
    time_max: String,
}

impl CalendarDayWindow {
    fn for_today(time_zone: &str) -> Result<Self, CalendarError> {
        Self::for_timestamp(time_zone, Timestamp::now())
    }

    fn for_timestamp(time_zone: &str, now: Timestamp) -> Result<Self, CalendarError> {
        let today = now
            .in_tz(time_zone)
            .map_err(CalendarError::InvalidTime)?
            .date();
        let start = today.in_tz(time_zone).map_err(CalendarError::InvalidTime)?;
        let tomorrow = today.tomorrow().map_err(CalendarError::InvalidTime)?;
        let end = tomorrow
            .in_tz(time_zone)
            .map_err(CalendarError::InvalidTime)?;
        let start_timestamp = start.timestamp();
        let end_timestamp = end.timestamp();

        Ok(Self {
            time_zone: time_zone.to_owned(),
            today,
            start_timestamp,
            end_timestamp,
            time_min: start_timestamp.to_string(),
            time_max: end_timestamp.to_string(),
        })
    }
}

#[derive(Debug, Deserialize)]
struct EventsListResponse {
    items: Option<Vec<GoogleEvent>>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoogleEvent {
    id: Option<String>,
    summary: Option<String>,
    #[serde(rename = "iCalUID")]
    i_cal_uid: Option<String>,
    start: Option<GoogleEventTime>,
    end: Option<GoogleEventTime>,
}

#[derive(Debug, Clone, Deserialize)]
struct GoogleEventTime {
    date: Option<String>,
    #[serde(rename = "dateTime")]
    date_time: Option<String>,
}

fn normalize_events(
    events: Vec<GoogleEvent>,
    window: &CalendarDayWindow,
) -> Result<Vec<CalendarDisplayEvent>, CalendarError> {
    let mut normalized = events
        .into_iter()
        .filter_map(|event| normalize_event(event, window).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen = HashSet::new();

    normalized.retain(|event| seen.insert(event.dedupe_key.clone()));
    normalized.sort_by(|left, right| {
        left.sort_group
            .cmp(&right.sort_group)
            .then_with(|| left.sort_seconds.cmp(&right.sort_seconds))
            .then_with(|| left.title.cmp(&right.title))
    });

    Ok(normalized)
}

fn normalize_event(
    event: GoogleEvent,
    window: &CalendarDayWindow,
) -> Result<Option<CalendarDisplayEvent>, CalendarError> {
    let Some(start) = event.start.as_ref() else {
        return Ok(None);
    };

    if start.date.is_some() {
        return normalize_all_day_event(event, window).map(Some);
    }
    if start.date_time.is_some() {
        return normalize_timed_event(event, window).map(Some);
    }

    Ok(None)
}

fn normalize_all_day_event(
    event: GoogleEvent,
    window: &CalendarDayWindow,
) -> Result<CalendarDisplayEvent, CalendarError> {
    let start_date = parse_event_date(
        event
            .start
            .as_ref()
            .and_then(|start| start.date.as_deref())
            .ok_or(CalendarError::InvalidCalendarData(
                "all-day event missing start date",
            ))?,
    )?;
    let end_date = parse_event_date(
        event
            .end
            .as_ref()
            .and_then(|end| end.date.as_deref())
            .ok_or(CalendarError::InvalidCalendarData(
                "all-day event missing end date",
            ))?,
    )?;

    if !(start_date <= window.today && end_date > window.today) {
        return Err(CalendarError::InvalidCalendarData(
            "all-day event did not overlap the requested day",
        ));
    }

    let title = event_title(event.summary.as_deref());
    let start_key = start_date.to_string();
    Ok(CalendarDisplayEvent {
        prefix: "ALL DAY".to_owned(),
        title,
        sort_group: 0,
        sort_seconds: 0,
        dedupe_key: dedupe_key(&event, &start_key),
    })
}

fn normalize_timed_event(
    event: GoogleEvent,
    window: &CalendarDayWindow,
) -> Result<CalendarDisplayEvent, CalendarError> {
    let start_timestamp = parse_event_timestamp(
        event
            .start
            .as_ref()
            .and_then(|start| start.date_time.as_deref())
            .ok_or(CalendarError::InvalidCalendarData(
                "timed event missing start time",
            ))?,
    )?;
    let display_timestamp = if start_timestamp < window.start_timestamp {
        window.start_timestamp
    } else {
        start_timestamp
    };
    if display_timestamp >= window.end_timestamp {
        return Err(CalendarError::InvalidCalendarData(
            "timed event did not overlap the requested day",
        ));
    }

    let prefix = display_timestamp
        .in_tz(&window.time_zone)
        .map_err(CalendarError::InvalidTime)?
        .strftime("%H:%M")
        .to_string();
    let title = event_title(event.summary.as_deref());
    let start_key = start_timestamp.to_string();

    Ok(CalendarDisplayEvent {
        prefix,
        title,
        sort_group: 1,
        sort_seconds: display_timestamp.as_second(),
        dedupe_key: dedupe_key(&event, &start_key),
    })
}

fn event_title(summary: Option<&str>) -> String {
    summary
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
        .unwrap_or("Untitled event")
        .to_owned()
}

fn dedupe_key(event: &GoogleEvent, start_key: &str) -> String {
    let event_key = event
        .i_cal_uid
        .as_deref()
        .or(event.id.as_deref())
        .unwrap_or("missing-event-id");

    format!("{event_key}\0{start_key}")
}

fn parse_event_date(date: &str) -> Result<Date, CalendarError> {
    date.parse::<Date>().map_err(CalendarError::InvalidTime)
}

fn parse_event_timestamp(timestamp: &str) -> Result<Timestamp, CalendarError> {
    timestamp
        .parse::<Timestamp>()
        .map_err(CalendarError::InvalidTime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GoogleCalendarConfig;

    #[test]
    fn builds_oauth_refresh_token_request_without_network() {
        let client = GoogleCalendarClient::new(&test_google_config());

        let request = client
            .build_refresh_token_request()
            .expect("request should build");

        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().as_str(), OAUTH_TOKEN_URL);
        let body = request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .expect("form body should be buffered");
        let body = std::str::from_utf8(body).expect("form body should be UTF-8");

        assert!(body.contains("client_id=google-client-id"));
        assert!(body.contains("client_secret=google-client-secret"));
        assert!(body.contains("refresh_token=google-refresh-token"));
        assert!(body.contains("grant_type=refresh_token"));
    }

    #[test]
    fn calendar_list_filtering_finds_primary_timezone_and_selected_visible_calendars() {
        let calendars = vec![
            CalendarListEntry {
                id: Some("primary@example.test".to_owned()),
                time_zone: Some("America/New_York".to_owned()),
                selected: Some(true),
                hidden: Some(false),
                primary: Some(true),
            },
            CalendarListEntry {
                id: Some("hidden@example.test".to_owned()),
                time_zone: Some("UTC".to_owned()),
                selected: Some(true),
                hidden: Some(true),
                primary: Some(false),
            },
            CalendarListEntry {
                id: Some("unselected@example.test".to_owned()),
                time_zone: Some("UTC".to_owned()),
                selected: Some(false),
                hidden: Some(false),
                primary: Some(false),
            },
        ];

        assert_eq!(
            primary_calendar_time_zone(&calendars).expect("timezone should exist"),
            "America/New_York"
        );
        assert_eq!(
            selected_visible_calendars(&calendars),
            [CalendarRef {
                id: "primary@example.test".to_owned()
            }]
        );
    }

    #[test]
    fn normalizes_sorts_deduplicates_and_formats_events() {
        let window = CalendarDayWindow::for_timestamp(
            "America/New_York",
            "2026-06-21T12:00:00Z"
                .parse()
                .expect("timestamp should parse"),
        )
        .expect("window should build");
        let events = vec![
            timed_event(
                "late",
                "late-uid",
                "2026-06-21T21:30:00-04:00",
                Some("Dinner"),
            ),
            all_day_event(
                "all",
                "all-uid",
                "2026-06-21",
                "2026-06-22",
                Some("Holiday"),
            ),
            timed_event("early", "early-uid", "2026-06-21T08:05:00-04:00", None),
            timed_event(
                "dupe",
                "early-uid",
                "2026-06-21T08:05:00-04:00",
                Some("Duplicate"),
            ),
            timed_event(
                "overnight",
                "overnight-uid",
                "2026-06-20T23:30:00-04:00",
                Some("Watch"),
            ),
        ];

        let display_events = normalize_events(events, &window).expect("events should normalize");

        assert_eq!(
            display_events
                .iter()
                .map(|event| (event.prefix(), event.title()))
                .collect::<Vec<_>>(),
            [
                ("ALL DAY", "Holiday"),
                ("00:00", "Watch"),
                ("08:05", "Untitled event"),
                ("21:30", "Dinner"),
            ]
        );
    }

    #[test]
    fn parses_paginated_calendar_and_event_response_shapes() {
        let calendar_response: CalendarListResponse = serde_json::from_str(
            r#"{
                "items": [{
                    "id": "primary@example.test",
                    "timeZone": "Europe/London",
                    "selected": true,
                    "primary": true
                }],
                "nextPageToken": "next-calendars"
            }"#,
        )
        .expect("calendar list response should parse");
        let events_response: EventsListResponse = serde_json::from_str(
            r#"{
                "items": [{
                    "id": "event-id",
                    "iCalUID": "event-uid",
                    "summary": "Meeting",
                    "start": { "dateTime": "2026-06-21T09:00:00+01:00" },
                    "end": { "dateTime": "2026-06-21T10:00:00+01:00" }
                }],
                "nextPageToken": "next-events"
            }"#,
        )
        .expect("events response should parse");

        assert_eq!(
            calendar_response.next_page_token.as_deref(),
            Some("next-calendars")
        );
        assert_eq!(
            events_response.next_page_token.as_deref(),
            Some("next-events")
        );
        assert_eq!(events_response.items.expect("items should exist").len(), 1);
    }

    fn test_google_config() -> GoogleCalendarConfig {
        GoogleCalendarConfig {
            client_id: "google-client-id".to_owned(),
            client_secret: SecretString::from_test_value("google-client-secret"),
            refresh_token: SecretString::from_test_value("google-refresh-token"),
        }
    }

    fn all_day_event(
        id: &str,
        uid: &str,
        start: &str,
        end: &str,
        summary: Option<&str>,
    ) -> GoogleEvent {
        GoogleEvent {
            id: Some(id.to_owned()),
            summary: summary.map(str::to_owned),
            i_cal_uid: Some(uid.to_owned()),
            start: Some(GoogleEventTime {
                date: Some(start.to_owned()),
                date_time: None,
            }),
            end: Some(GoogleEventTime {
                date: Some(end.to_owned()),
                date_time: None,
            }),
        }
    }

    fn timed_event(id: &str, uid: &str, start: &str, summary: Option<&str>) -> GoogleEvent {
        GoogleEvent {
            id: Some(id.to_owned()),
            summary: summary.map(str::to_owned),
            i_cal_uid: Some(uid.to_owned()),
            start: Some(GoogleEventTime {
                date: None,
                date_time: Some(start.to_owned()),
            }),
            end: None,
        }
    }
}
