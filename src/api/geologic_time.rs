//! Eras and periods (spec §5.1, §5.2).

use actix_web::{HttpRequest, HttpResponse, web};
use bigdecimal::BigDecimal;
use serde::Serialize;
use sqlx::{PgConnection, PgPool, Postgres, QueryBuilder};

use super::db_error::{Ctx, Op, map_write};
use super::decimal::Decimal;
use super::error::{ApiError, FieldError, Resource};
use super::http::{Many, One, cached_json, created, updated};
use super::input::{self, Obj, decimal, is_slug, slug};
use super::params::{ListQuery, Pagination, Params, Sort};
use super::species::card::Summary;
use super::{begin_write, delete_resource, paged, read_tx, write_body};

// ---------------------------------------------------------------- representations

/// An era (spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Era {
    pub id: String,
    pub name: String,
    pub start_mya: Decimal,
    pub end_mya: Decimal,
}

/// A period with its era summary (spec §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Period {
    pub id: String,
    pub name: String,
    pub start_mya: Decimal,
    pub end_mya: Decimal,
    pub era: Summary,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct EraRow {
    id: String,
    name: String,
    start_mya: BigDecimal,
    end_mya: BigDecimal,
}

impl From<EraRow> for Era {
    fn from(r: EraRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            start_mya: Decimal::from_db(r.start_mya),
            end_mya: Decimal::from_db(r.end_mya),
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct PeriodRow {
    id: String,
    name: String,
    start_mya: BigDecimal,
    end_mya: BigDecimal,
    era_id: String,
    era_name: String,
}

impl From<PeriodRow> for Period {
    fn from(r: PeriodRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            start_mya: Decimal::from_db(r.start_mya),
            end_mya: Decimal::from_db(r.end_mya),
            era: Summary {
                id: r.era_id,
                name: r.era_name,
            },
        }
    }
}

fixed_query! {
    /// Q-E3: one era by id.
    pub const ERA_BY_ID_SQL = "SELECT e.id, e.name, e.start_mya, e.end_mya FROM eras e WHERE e.id = $1";
    pub(crate) fn era_by_id(id: &str) -> fetch_optional EraRow;
}

fixed_query! {
    /// Q-P4: one period by id with its era (PK join).
    pub const PERIOD_BY_ID_SQL = "SELECT p.id, p.name, p.start_mya, p.end_mya, e.id AS era_id, e.name AS era_name \
        FROM periods p JOIN eras e ON e.id = p.era_id WHERE p.id = $1";
    pub(crate) fn period_by_id(id: &str) -> fetch_optional PeriodRow;
}

// ---------------------------------------------------------------- list SQL

/// Sorts of eras and periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeSort {
    StartMya,
    Name,
}

pub const SORTS: &[(&str, TimeSort)] =
    &[("start_mya", TimeSort::StartMya), ("name", TimeSort::Name)];

/// `-start_mya`: oldest first (AC 6.2.6).
pub const DEFAULT_SORT: Sort<TimeSort> = Sort {
    key: TimeSort::StartMya,
    desc: true,
};

/// Era list: no filters.
pub type EraQuery = ListQuery<(), TimeSort>;

/// Period list: optional era (`/periods?era=`, `/eras/{era_id}/periods`).
pub type PeriodQuery = ListQuery<Option<String>, TimeSort>;

fn push_order(qb: &mut QueryBuilder<Postgres>, alias: &str, sort: Sort<TimeSort>) {
    let dir = if sort.desc { " DESC" } else { "" };
    match sort.key {
        TimeSort::StartMya => qb.push(format_args!(" ORDER BY {alias}.start_mya{dir}, {alias}.id")),
        TimeSort::Name => qb.push(format_args!(
            " ORDER BY lower({alias}.name) COLLATE paleo_name_sort{dir}, {alias}.id"
        )),
    };
}

fn push_limit<F, S>(qb: &mut QueryBuilder<Postgres>, q: &ListQuery<F, S>) {
    qb.push(" LIMIT ")
        .push_bind(i64::from(q.page.limit))
        .push("::bigint OFFSET ")
        .push_bind(q.page.offset())
        .push("::bigint");
}

/// Q-E1 / Q-E2 page statement.
pub fn push_era_page_sql(qb: &mut QueryBuilder<Postgres>, q: &EraQuery) {
    qb.push("SELECT e.id, e.name, e.start_mya, e.end_mya FROM eras e");
    push_order(qb, "e", q.sort);
    push_limit(qb, q);
}

/// Era count statement.
pub fn push_era_count_sql(qb: &mut QueryBuilder<Postgres>, _q: &EraQuery) {
    qb.push("SELECT count(*) FROM eras e");
}

fn push_period_where(qb: &mut QueryBuilder<Postgres>, era: &Option<String>) {
    if let Some(era) = era {
        qb.push(" WHERE p.era_id = ")
            .push_bind(era.clone())
            .push("::text");
    }
}

/// Q-P1…Q-P3 page statement, with the era joined on its PK.
pub fn push_period_page_sql(qb: &mut QueryBuilder<Postgres>, q: &PeriodQuery) {
    qb.push(
        "SELECT p.id, p.name, p.start_mya, p.end_mya, e.id AS era_id, e.name AS era_name \
         FROM periods p JOIN eras e ON e.id = p.era_id",
    );
    push_period_where(qb, &q.filter);
    push_order(qb, "p", q.sort);
    push_limit(qb, q);
}

/// Period count statement (no join).
pub fn push_period_count_sql(qb: &mut QueryBuilder<Postgres>, q: &PeriodQuery) {
    qb.push("SELECT count(*) FROM periods p");
    push_period_where(qb, &q.filter);
}

// ---------------------------------------------------------------- read handlers

fn page_and_sort(p: &Params) -> Result<(super::params::Page, Sort<TimeSort>), ApiError> {
    Ok((p.page()?, p.sort(SORTS, DEFAULT_SORT)?))
}

/// `GET /eras`.
pub async fn list_eras(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let p = Params::parse(req.query_string(), &["page", "limit", "sort"])?;
    let (page, sort) = page_and_sort(&p)?;
    let q = EraQuery {
        page,
        sort,
        filter: (),
    };
    let mut tx = read_tx(&pool).await?;
    let (total, rows) = paged::<EraRow>(
        &mut tx,
        q.page,
        |qb| push_era_count_sql(qb, &q),
        |qb| push_era_page_sql(qb, &q),
    )
    .await?;
    tx.commit().await?;
    Ok(cached_json(
        &req,
        &Many {
            data: rows.into_iter().map(Era::from).collect(),
            pagination: Pagination::new(q.page, total),
        },
    ))
}

/// `GET /eras/{era_id}`.
pub async fn get_era(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Era, &id));
    }
    let mut conn = pool.acquire().await?;
    match era_by_id(&mut conn, &id).await? {
        Some(row) => Ok(cached_json(
            &req,
            &One {
                data: Era::from(row),
            },
        )),
        None => Err(ApiError::not_found(Resource::Era, &id)),
    }
}

async fn list_periods_for(
    req: &HttpRequest,
    pool: &PgPool,
    q: PeriodQuery,
    parent: Option<&str>,
) -> Result<HttpResponse, ApiError> {
    let mut tx = read_tx(pool).await?;
    if let Some(era) = parent
        && era_by_id(&mut tx, era).await?.is_none()
    {
        return Err(ApiError::not_found(Resource::Era, era));
    }
    let (total, rows) = paged::<PeriodRow>(
        &mut tx,
        q.page,
        |qb| push_period_count_sql(qb, &q),
        |qb| push_period_page_sql(qb, &q),
    )
    .await?;
    tx.commit().await?;
    Ok(cached_json(
        req,
        &Many {
            data: rows.into_iter().map(Period::from).collect(),
            pagination: Pagination::new(q.page, total),
        },
    ))
}

/// `GET /periods`.
pub async fn list_periods(
    req: HttpRequest,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let p = Params::parse(req.query_string(), &["page", "limit", "sort", "era"])?;
    let (page, sort) = page_and_sort(&p)?;
    let q = PeriodQuery {
        page,
        sort,
        filter: p.slug("era")?,
    };
    list_periods_for(&req, &pool, q, None).await
}

/// `GET /eras/{era_id}/periods`.
pub async fn list_era_periods(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let p = Params::parse(req.query_string(), &["page", "limit", "sort"])?;
    let (page, sort) = page_and_sort(&p)?;
    let era = path.into_inner();
    if !is_slug(&era) {
        return Err(ApiError::not_found(Resource::Era, &era));
    }
    let q = PeriodQuery {
        page,
        sort,
        filter: Some(era.clone()),
    };
    list_periods_for(&req, &pool, q, Some(&era)).await
}

/// `GET /periods/{period_id}`.
pub async fn get_period(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Period, &id));
    }
    let mut conn = pool.acquire().await?;
    match period_by_id(&mut conn, &id).await? {
        Some(row) => Ok(cached_json(
            &req,
            &One {
                data: Period::from(row),
            },
        )),
        None => Err(ApiError::not_found(Resource::Period, &id)),
    }
}

// ---------------------------------------------------------------- range rules (pure)

/// Which range fields a request body sent, `null` included (data-model §6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sent {
    pub start_mya: bool,
    pub end_mya: bool,
    pub era_id: bool,
}

/// The resulting range: sent values over stored ones (spec §4.10).
pub fn merge_range(
    stored: (Decimal, Decimal),
    start: Option<Decimal>,
    end: Option<Decimal>,
) -> (Decimal, Decimal) {
    (start.unwrap_or(stored.0), end.unwrap_or(stored.1))
}

fn only_start(sent: Sent) -> bool {
    sent.start_mya && !sent.end_mya
}

fn only_end(sent: Sent) -> bool {
    sent.end_mya && !sent.start_mya
}

/// `start_mya > end_mya`, attributed per data-model §6.
pub fn range_problem(start: &Decimal, end: &Decimal, sent: Sent) -> Option<FieldError> {
    if start > end {
        return None;
    }
    Some(if only_start(sent) {
        FieldError::new(
            "start_mya",
            format!("start_mya ({start}) must be greater than end_mya ({end})."),
        )
    } else {
        FieldError::new(
            "end_mya",
            format!("end_mya ({end}) must be less than start_mya ({start})."),
        )
    })
}

/// Field of an era or period overlap (data-model §6).
pub fn overlap_field(sent: Sent) -> &'static str {
    if only_end(sent) {
        "end_mya"
    } else {
        "start_mya"
    }
}

/// Fields of a period outside its era (data-model §6); empty when inside.
pub fn outside_era_fields(
    period: (&Decimal, &Decimal),
    era: (&Decimal, &Decimal),
    sent: Sent,
) -> Vec<&'static str> {
    let starts_before = period.0 > era.0;
    let ends_after = period.1 < era.1;
    if !starts_before && !ends_after {
        return Vec::new();
    }
    if sent.era_id && !sent.start_mya && !sent.end_mya {
        return vec!["era_id"];
    }
    let mut fields = Vec::new();
    if starts_before {
        fields.push("start_mya");
    }
    if ends_after {
        fields.push("end_mya");
    }
    fields
}

/// Sides of an era range that exclude some of its periods (data-model §6).
pub fn excluded_sides(
    era: (&Decimal, &Decimal),
    periods: &[(Decimal, Decimal)],
) -> Vec<&'static str> {
    let mut sides = Vec::new();
    if periods.iter().any(|(s, _)| s > era.0) {
        sides.push("start_mya");
    }
    if periods.iter().any(|(_, e)| e < era.1) {
        sides.push("end_mya");
    }
    sides
}

// ---------------------------------------------------------------- write lookups (data-model §3)

#[derive(Debug)]
struct IdRow {
    id: String,
}

#[derive(Debug)]
struct OutsideRow {
    id: String,
    start_mya: BigDecimal,
    end_mya: BigDecimal,
}

fixed_query! {
    /// Q-W5: eras overlapping a range, other than `$3` (first 5 by id).
    pub const ERA_OVERLAP_SQL = "SELECT e.id FROM eras e \
        WHERE numrange(e.end_mya, e.start_mya, '[)') && numrange($1::numeric, $2::numeric, '[)') \
          AND e.id <> $3 ORDER BY e.id LIMIT 5";
    fn era_overlaps(end: &BigDecimal, start: &BigDecimal, id: &str) -> fetch_all IdRow;
}

fixed_query! {
    /// Q-W6: periods overlapping a range, other than `$3` (first 5 by id).
    pub const PERIOD_OVERLAP_SQL = "SELECT p.id FROM periods p \
        WHERE numrange(p.end_mya, p.start_mya, '[)') && numrange($1::numeric, $2::numeric, '[)') \
          AND p.id <> $3 ORDER BY p.id LIMIT 5";
    fn period_overlaps(end: &BigDecimal, start: &BigDecimal, id: &str) -> fetch_all IdRow;
}

fixed_query! {
    /// Q-W7: periods of an era outside a new era range (first 5 by id).
    pub const ERA_PERIODS_OUTSIDE_SQL = "SELECT p.id, p.start_mya, p.end_mya FROM periods p \
        WHERE p.era_id = $1 AND (p.start_mya > $2::numeric OR p.end_mya < $3::numeric) \
        ORDER BY p.id LIMIT 5";
    fn era_periods_outside(id: &str, start: &BigDecimal, end: &BigDecimal) -> fetch_all OutsideRow;
}

fn mya() -> impl Fn(&str, &serde_json::Value) -> Result<Decimal, String> {
    decimal(3, 0, false, Some(4600))
}

fn id_list(rows: &[IdRow]) -> String {
    rows.iter()
        .map(|r| r.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

const ID_IMMUTABLE: &str =
    "id cannot be changed. Remove it from the body; create a new resource instead.";

/// Range rules of an era with the resulting range: order, overlap, containment.
async fn check_era_range(
    conn: &mut PgConnection,
    o: &mut Obj,
    id: &str,
    range: (&Decimal, &Decimal),
    sent: Sent,
    existing: bool,
) -> sqlx::Result<()> {
    if let Some(e) = range_problem(range.0, range.1, sent) {
        o.push(e.field, e.message);
        return Ok(());
    }
    let overlaps = era_overlaps(conn, range.1.as_big(), range.0.as_big(), id).await?;
    if !overlaps.is_empty() {
        o.push(
            overlap_field(sent),
            format!(
                "The range {}–{} overlaps era(s) {}. Eras may touch at a boundary but not overlap.",
                range.0,
                range.1,
                id_list(&overlaps)
            ),
        );
    }
    if existing {
        let outside = era_periods_outside(conn, id, range.0.as_big(), range.1.as_big()).await?;
        let ranges: Vec<(Decimal, Decimal)> = outside
            .iter()
            .map(|r| {
                (
                    Decimal::from_db(r.start_mya.clone()),
                    Decimal::from_db(r.end_mya.clone()),
                )
            })
            .collect();
        let ids: Vec<&str> = outside.iter().map(|r| r.id.as_str()).collect();
        for side in excluded_sides(range, &ranges) {
            o.push(
                side,
                format!(
                    "The range {}–{} must still contain all of the era's periods; it leaves out {}. \
                     Change or move those periods first.",
                    range.0,
                    range.1,
                    ids.join(", ")
                ),
            );
        }
    }
    Ok(())
}

/// Range rules of a period with the resulting range and era.
async fn check_period_range(
    conn: &mut PgConnection,
    o: &mut Obj,
    id: &str,
    range: (&Decimal, &Decimal),
    era: Option<(&str, &Decimal, &Decimal)>,
    sent: Sent,
) -> sqlx::Result<()> {
    if let Some(e) = range_problem(range.0, range.1, sent) {
        o.push(e.field, e.message);
        return Ok(());
    }
    if let Some((era_id, es, ee)) = era {
        for field in outside_era_fields(range, (es, ee), sent) {
            let message = match field {
                "era_id" => format!(
                    "The period's range {}–{} does not lie within era '{era_id}' ({es}–{ee}). \
                     Send a new range together with era_id, or choose another era.",
                    range.0, range.1
                ),
                "start_mya" => format!(
                    "start_mya ({}) must not be greater than the start_mya of era '{era_id}' ({es}).",
                    range.0
                ),
                _ => format!(
                    "end_mya ({}) must not be less than the end_mya of era '{era_id}' ({ee}).",
                    range.1
                ),
            };
            o.push(field, message);
        }
    }
    if sent.start_mya || sent.end_mya {
        let overlaps = period_overlaps(conn, range.1.as_big(), range.0.as_big(), id).await?;
        if !overlaps.is_empty() {
            o.push(
                overlap_field(sent),
                format!(
                    "The range {}–{} overlaps period(s) {}. Periods may touch at a boundary but not overlap.",
                    range.0,
                    range.1,
                    id_list(&overlaps)
                ),
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- write handlers

/// `POST /eras`.
pub async fn create_era(
    req: HttpRequest,
    payload: web::Payload,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let body = write_body(&req, payload).await?;
    let mut tx = begin_write(&pool).await?;
    let mut o = Obj::new(body)?;
    let id = o.required("id", slug);
    let name = o.required("name", input::name);
    let start = o.required("start_mya", mya());
    let end = o.required("end_mya", mya());
    let sent = Sent {
        start_mya: true,
        end_mya: true,
        era_id: false,
    };
    if let (Some(s), Some(e)) = (&start, &end) {
        check_era_range(
            &mut tx,
            &mut o,
            id.as_deref().unwrap_or(""),
            (s, e),
            sent,
            false,
        )
        .await?;
    }
    o.finish()?;
    let (Some(id), Some(name), Some(start), Some(end)) = (id, name, start, end) else {
        return Err(ApiError::internal());
    };
    let ctx = Ctx::new(Resource::Era, &id, Op::Insert)
        .name(Some(&name))
        .sent(sent);
    sqlx::query!(
        "INSERT INTO eras (id, name, start_mya, end_mya) VALUES ($1, $2, $3, $4)",
        id,
        name,
        start.as_big(),
        end.as_big()
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    let row = era_by_id(&mut tx, &id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await.map_err(|e| map_write(&e, &ctx))?;
    Ok(created(
        format!("/api/v1/eras/{id}"),
        &One {
            data: Era::from(row),
        },
    ))
}

/// `PATCH /eras/{era_id}`.
pub async fn update_era(
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Payload,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let body = write_body(&req, payload).await?;
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Era, &id));
    }
    let mut tx = begin_write(&pool).await?;
    let Some(stored) = sqlx::query!(
        "SELECT start_mya, end_mya FROM eras WHERE id = $1 FOR NO KEY UPDATE",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Err(ApiError::not_found(Resource::Era, &id));
    };
    let mut o = Obj::new(body)?;
    let sent = Sent {
        start_mya: o.has("start_mya"),
        end_mya: o.has("end_mya"),
        era_id: false,
    };
    o.forbid("id", ID_IMMUTABLE);
    o.require_some_field();
    let name = o.patch("name", input::name);
    let start = o.patch("start_mya", mya());
    let end = o.patch("end_mya", mya());
    if let (Some(start), Some(end)) = (&start, &end)
        && (sent.start_mya || sent.end_mya)
    {
        let (s, e) = merge_range(
            (
                Decimal::from_db(stored.start_mya),
                Decimal::from_db(stored.end_mya),
            ),
            start.clone(),
            end.clone(),
        );
        check_era_range(&mut tx, &mut o, &id, (&s, &e), sent, true).await?;
    }
    o.finish()?;
    let name = name.flatten();
    let (start, end) = (start.flatten(), end.flatten());
    let ctx = Ctx::new(Resource::Era, &id, Op::Update)
        .name(name.as_deref())
        .sent(sent);
    sqlx::query!(
        "UPDATE eras SET name = COALESCE($2, name), start_mya = COALESCE($3, start_mya), \
         end_mya = COALESCE($4, end_mya) WHERE id = $1",
        id,
        name,
        start.as_ref().map(Decimal::as_big),
        end.as_ref().map(Decimal::as_big)
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    let row = era_by_id(&mut tx, &id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await.map_err(|e| map_write(&e, &ctx))?;
    Ok(updated(&One {
        data: Era::from(row),
    }))
}

/// `DELETE /eras/{era_id}`.
pub async fn delete_era(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    delete_resource(
        &req,
        &pool,
        Resource::Era,
        &id,
        async |c: &mut PgConnection| {
            Ok(
                sqlx::query!("SELECT 1 AS one FROM eras WHERE id = $1 FOR UPDATE", id)
                    .fetch_optional(c)
                    .await?
                    .is_some(),
            )
        },
        async |c: &mut PgConnection| {
            sqlx::query!("DELETE FROM eras WHERE id = $1", id)
                .execute(c)
                .await
                .map(|_| ())
        },
    )
    .await
}

/// `POST /eras/{era_id}/periods`.
pub async fn create_period(
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Payload,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let body = write_body(&req, payload).await?;
    let era_id = path.into_inner();
    if !is_slug(&era_id) {
        return Err(ApiError::not_found(Resource::Era, &era_id));
    }
    let mut tx = begin_write(&pool).await?;
    let Some(era) = era_by_id(&mut tx, &era_id).await? else {
        return Err(ApiError::not_found(Resource::Era, &era_id));
    };
    let era = Era::from(era);
    let mut o = Obj::new(body)?;
    o.forbid(
        "era_id",
        "era_id must not be sent: the period is created in the era of the path.",
    );
    let id = o.required("id", slug);
    let name = o.required("name", input::name);
    let start = o.required("start_mya", mya());
    let end = o.required("end_mya", mya());
    let sent = Sent {
        start_mya: true,
        end_mya: true,
        era_id: false,
    };
    if let (Some(s), Some(e)) = (&start, &end) {
        check_period_range(
            &mut tx,
            &mut o,
            id.as_deref().unwrap_or(""),
            (s, e),
            Some((&era.id, &era.start_mya, &era.end_mya)),
            sent,
        )
        .await?;
    }
    o.finish()?;
    let (Some(id), Some(name), Some(start), Some(end)) = (id, name, start, end) else {
        return Err(ApiError::internal());
    };
    let ctx = Ctx::new(Resource::Period, &id, Op::Insert)
        .name(Some(&name))
        .sent(sent);
    sqlx::query!(
        "INSERT INTO periods (id, era_id, name, start_mya, end_mya) VALUES ($1, $2, $3, $4, $5)",
        id,
        era_id,
        name,
        start.as_big(),
        end.as_big()
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    let row = period_by_id(&mut tx, &id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await.map_err(|e| map_write(&e, &ctx))?;
    Ok(created(
        format!("/api/v1/periods/{id}"),
        &One {
            data: Period::from(row),
        },
    ))
}

/// `PATCH /periods/{period_id}`.
pub async fn update_period(
    req: HttpRequest,
    path: web::Path<String>,
    payload: web::Payload,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let body = write_body(&req, payload).await?;
    let id = path.into_inner();
    if !is_slug(&id) {
        return Err(ApiError::not_found(Resource::Period, &id));
    }
    let mut tx = begin_write(&pool).await?;
    let Some(stored) = sqlx::query!(
        "SELECT era_id, start_mya, end_mya FROM periods WHERE id = $1 FOR NO KEY UPDATE",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Err(ApiError::not_found(Resource::Period, &id));
    };
    let mut o = Obj::new(body)?;
    let sent = Sent {
        start_mya: o.has("start_mya"),
        end_mya: o.has("end_mya"),
        era_id: o.has("era_id"),
    };
    o.forbid("id", ID_IMMUTABLE);
    o.require_some_field();
    let name = o.patch("name", input::name);
    let start = o.patch("start_mya", mya());
    let end = o.patch("end_mya", mya());
    let era_id = o.patch("era_id", slug);

    // The resulting era: the sent one (it must exist) or the stored one.
    let target = match &era_id {
        Some(Some(new)) => Some(new.clone()),
        Some(None) => Some(stored.era_id.clone()),
        None => None,
    };
    let era = match &target {
        Some(e) => era_by_id(&mut tx, e).await?.map(Era::from),
        None => None,
    };
    if let (Some(Some(new)), None) = (&era_id, &era) {
        o.push(
            "era_id",
            format!(
                "era_id '{}' does not exist. Send the id of an existing era.",
                input::truncate(new, input::ECHO)
            ),
        );
    }
    if let (Some(start), Some(end)) = (&start, &end)
        && (sent.start_mya || sent.end_mya || sent.era_id)
    {
        let (s, e) = merge_range(
            (
                Decimal::from_db(stored.start_mya),
                Decimal::from_db(stored.end_mya),
            ),
            start.clone(),
            end.clone(),
        );
        let era = era
            .as_ref()
            .map(|e| (e.id.as_str(), &e.start_mya, &e.end_mya));
        check_period_range(&mut tx, &mut o, &id, (&s, &e), era, sent).await?;
    }
    o.finish()?;
    let name = name.flatten();
    let (start, end) = (start.flatten(), end.flatten());
    let ctx = Ctx::new(Resource::Period, &id, Op::Update)
        .name(name.as_deref())
        .sent(sent);
    sqlx::query!(
        "UPDATE periods SET name = COALESCE($2, name), era_id = COALESCE($3, era_id), \
         start_mya = COALESCE($4, start_mya), end_mya = COALESCE($5, end_mya) WHERE id = $1",
        id,
        name,
        era_id.flatten(),
        start.as_ref().map(Decimal::as_big),
        end.as_ref().map(Decimal::as_big)
    )
    .execute(&mut *tx)
    .await
    .map_err(|e| map_write(&e, &ctx))?;
    let row = period_by_id(&mut tx, &id)
        .await?
        .ok_or_else(ApiError::internal)?;
    tx.commit().await.map_err(|e| map_write(&e, &ctx))?;
    Ok(updated(&One {
        data: Period::from(row),
    }))
}

/// `DELETE /periods/{period_id}`.
pub async fn delete_period(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let id = path.into_inner();
    delete_resource(
        &req,
        &pool,
        Resource::Period,
        &id,
        async |c: &mut PgConnection| {
            Ok(
                sqlx::query!("SELECT 1 AS one FROM periods WHERE id = $1 FOR UPDATE", id)
                    .fetch_optional(c)
                    .await?
                    .is_some(),
            )
        },
        async |c: &mut PgConnection| {
            sqlx::query!("DELETE FROM periods WHERE id = $1", id)
                .execute(c)
                .await
                .map(|_| ())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn d(text: &str) -> Decimal {
        Decimal::parse(&serde_json::Number::from_str(text).unwrap(), 3).unwrap()
    }

    const NONE: Sent = Sent {
        start_mya: false,
        end_mya: false,
        era_id: false,
    };
    const START: Sent = Sent {
        start_mya: true,
        ..NONE
    };
    const END: Sent = Sent {
        end_mya: true,
        ..NONE
    };
    const BOTH: Sent = Sent {
        start_mya: true,
        end_mya: true,
        era_id: false,
    };
    const ERA: Sent = Sent {
        era_id: true,
        ..NONE
    };
    const ERA_START: Sent = Sent {
        era_id: true,
        start_mya: true,
        end_mya: false,
    };

    #[test]
    fn merged_range_takes_sent_values_over_stored() {
        let stored = (d("201.4"), d("145"));
        assert_eq!(merge_range(stored.clone(), None, None), stored);
        assert_eq!(
            merge_range(stored.clone(), Some(d("200")), None),
            (d("200"), d("145"))
        );
        assert_eq!(
            merge_range(stored.clone(), None, Some(d("150"))),
            (d("201.4"), d("150"))
        );
        assert_eq!(
            merge_range(stored, Some(d("10")), Some(d("5"))),
            (d("10"), d("5"))
        );
    }

    #[test]
    fn start_must_be_greater_than_end_after_merge() {
        assert_eq!(range_problem(&d("201.4"), &d("145"), BOTH), None);
        for (start, end) in [("145", "145"), ("145", "201.4")] {
            let e = range_problem(&d(start), &d(end), BOTH).unwrap();
            assert_eq!(e.field, "end_mya");
            assert!(
                e.message.contains(start) && e.message.contains(end),
                "{}",
                e.message
            );
            assert_eq!(
                range_problem(&d(start), &d(end), END).unwrap().field,
                "end_mya"
            );
            assert_eq!(
                range_problem(&d(start), &d(end), START).unwrap().field,
                "start_mya"
            );
            assert_eq!(
                range_problem(&d(start), &d(end), ERA_START).unwrap().field,
                "start_mya"
            );
            assert_eq!(
                range_problem(&d(start), &d(end), NONE).unwrap().field,
                "end_mya"
            );
        }
    }

    #[test]
    fn overlap_attribution() {
        assert_eq!(overlap_field(BOTH), "start_mya");
        assert_eq!(overlap_field(START), "start_mya");
        assert_eq!(overlap_field(END), "end_mya");
        assert_eq!(overlap_field(ERA), "start_mya");
        assert_eq!(overlap_field(NONE), "start_mya");
    }

    #[test]
    fn period_outside_its_era() {
        let era = (&d("251.902"), &d("66"));
        let inside = [("251.902", "66"), ("200", "100")];
        for (s, e) in inside {
            for sent in [NONE, START, END, BOTH, ERA, ERA_START] {
                assert!(outside_era_fields((&d(s), &d(e)), era, sent).is_empty());
            }
        }
        // only era_id sent: the move is the problem
        assert_eq!(
            outside_era_fields((&d("300"), &d("10")), era, ERA),
            ["era_id"]
        );
        // range fields sent: one detail per violated side
        for sent in [START, END, BOTH, ERA_START, NONE] {
            assert_eq!(
                outside_era_fields((&d("300"), &d("100")), era, sent),
                ["start_mya"]
            );
            assert_eq!(
                outside_era_fields((&d("200"), &d("10")), era, sent),
                ["end_mya"]
            );
            assert_eq!(
                outside_era_fields((&d("300"), &d("10")), era, sent),
                ["start_mya", "end_mya"]
            );
        }
    }

    #[test]
    fn era_excluding_its_periods() {
        let periods = [(d("251.902"), d("201.4")), (d("145"), d("66"))];
        assert!(excluded_sides((&d("251.902"), &d("66")), &periods).is_empty());
        assert!(excluded_sides((&d("260"), &d("60")), &periods).is_empty());
        assert_eq!(
            excluded_sides((&d("240"), &d("66")), &periods),
            ["start_mya"]
        );
        assert_eq!(
            excluded_sides((&d("251.902"), &d("100")), &periods),
            ["end_mya"]
        );
        assert_eq!(
            excluded_sides((&d("240"), &d("100")), &periods),
            ["start_mya", "end_mya"]
        );
        assert!(excluded_sides((&d("10"), &d("5")), &[]).is_empty());
    }
}
