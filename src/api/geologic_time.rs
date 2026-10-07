//! Eras and periods (spec §5.1, §5.2).

use actix_web::{HttpRequest, HttpResponse, web};
use bigdecimal::BigDecimal;
use serde::Serialize;
use sqlx::{PgPool, Postgres, QueryBuilder};

use super::decimal::Decimal;
use super::error::{ApiError, FieldError, Resource};
use super::http::{Many, One, cached_json};
use super::input::is_slug;
use super::params::{ListQuery, Pagination, Params, Sort};
use super::species::card::Summary;
use super::{paged, read_tx};

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
    _stored: (Decimal, Decimal),
    _start: Option<Decimal>,
    _end: Option<Decimal>,
) -> (Decimal, Decimal) {
    todo!()
}

/// `start_mya > end_mya`, attributed per data-model §6.
pub fn range_problem(_start: &Decimal, _end: &Decimal, _sent: Sent) -> Option<FieldError> {
    todo!()
}

/// Field of an era or period overlap (data-model §6).
pub fn overlap_field(_sent: Sent) -> &'static str {
    todo!()
}

/// Fields of a period outside its era (data-model §6); empty when inside.
pub fn outside_era_fields(
    _period: (&Decimal, &Decimal),
    _era: (&Decimal, &Decimal),
    _sent: Sent,
) -> Vec<&'static str> {
    todo!()
}

/// Sides of an era range that exclude some of its periods (data-model §6).
pub fn excluded_sides(
    _era: (&Decimal, &Decimal),
    _periods: &[(Decimal, Decimal)],
) -> Vec<&'static str> {
    todo!()
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
