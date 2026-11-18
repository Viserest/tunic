//! # Food
//!
//! Track nutrition and workouts.
//!
//! * [`Ingredient`]: nutrition per serving (e.g. *Yogurt, 1 cup: 90 kcal, 18 g protein ...*)
//! * [`Meal`]: named collection of ingredients with serving counts
//! * [`Eaten`]: something eaten at a time (a meal or a single ingredient)
//! * [`Workout`]: exercise with calories burned
//! * `GET /food/summary?day=YYYY-MM-DD` totals for a day (UTC)

use database::{Database, DbError, Record, Table, Uuid, Value};
use element::{Page, heading, table, text};
use router::{Request, Response, Router};
use user::{Owned, Users, owned_crud};
use utils::{date_to_unix, json_obj, parse_date, unix_now, unix_to_date};

/// Nutrition of one serving. Names can distinguish cooked from uncooked ("Rice, cooked").
#[derive(Clone, Debug, PartialEq, Record)]
pub struct Ingredient {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    /// Serving size in grams (or ml).
    pub serving_size: f64,
    pub kcalories: f64,
    #[record(default)]
    pub protein: f64,
    #[record(default)]
    pub carbohydrates: f64,
    #[record(default)]
    pub fiber: f64,
    #[record(default)]
    pub sugar: f64,
    #[record(default)]
    pub fat: f64,
    #[record(default)]
    pub saturated_fat: f64,
    #[record(default)]
    pub trans_fat: f64,
    #[record(default)]
    pub created_at: u64,
    #[record(default)]
    pub updated_at: u64,
}

/// `items` is a list of `{"ingredient_id": uuid, "servings": number}`.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct Meal {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    pub items: Vec<Value>,
    #[record(default)]
    pub created_at: u64,
    #[record(default)]
    pub updated_at: u64,
}

/// Something eaten: a `meal_id` or an `ingredient_id`, times `servings`.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct Eaten {
    pub id: Uuid,
    pub owner: Uuid,
    pub ate_at: u64,
    pub meal_id: Option<Uuid>,
    pub ingredient_id: Option<Uuid>,
    #[record(default)]
    pub servings: f64,
    #[record(default)]
    pub created_at: u64,
    #[record(default)]
    pub updated_at: u64,
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Workout {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    pub performed_at: u64,
    #[record(default)]
    pub duration_minutes: f64,
    #[record(default)]
    pub kcalories_burned: f64,
    #[record(default)]
    pub notes: String,
    #[record(default)]
    pub created_at: u64,
    #[record(default)]
    pub updated_at: u64,
}

macro_rules! owned {
    ($($t:ty),*) => {$( impl Owned for $t { fn owner(&self) -> Uuid { self.owner } } )*};
}
owned!(Ingredient, Meal, Eaten, Workout);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Totals {
    pub kcalories: f64,
    pub protein: f64,
    pub carbohydrates: f64,
    pub fiber: f64,
    pub sugar: f64,
    pub fat: f64,
    pub saturated_fat: f64,
    pub trans_fat: f64,
}

impl Totals {
    fn add(&mut self, i: &Ingredient, servings: f64) {
        self.kcalories += i.kcalories * servings;
        self.protein += i.protein * servings;
        self.carbohydrates += i.carbohydrates * servings;
        self.fiber += i.fiber * servings;
        self.sugar += i.sugar * servings;
        self.fat += i.fat * servings;
        self.saturated_fat += i.saturated_fat * servings;
        self.trans_fat += i.trans_fat * servings;
    }

    pub fn to_value(&self) -> Value {
        let r = |x: f64| (x * 100.0).round() / 100.0;
        json_obj! {
            "kcalories" => r(self.kcalories), "protein" => r(self.protein), "carbohydrates" => r(self.carbohydrates),
            "fiber" => r(self.fiber), "sugar" => r(self.sugar), "fat" => r(self.fat),
            "saturated_fat" => r(self.saturated_fat), "trans_fat" => r(self.trans_fat),
        }
    }
}

#[derive(Clone)]
pub struct Food {
    pub ingredients: Table<Ingredient>,
    pub meals: Table<Meal>,
    pub eaten: Table<Eaten>,
    pub workouts: Table<Workout>,
}

impl Food {
    pub fn new(db: Database) -> Result<Food, DbError> {
        Ok(Food { ingredients: Table::new(db.clone())?, meals: Table::new(db.clone())?, eaten: Table::new(db.clone())?, workouts: Table::new(db)? })
    }

    /// Nutrition of a meal (every item's ingredient must belong to `owner`).
    pub fn meal_totals(&self, owner: &Uuid, meal: &Meal, multiplier: f64) -> Totals {
        let mut t = Totals::default();
        for item in &meal.items {
            let id = item.get("ingredient_id").and_then(|i| i.as_str()).and_then(Uuid::parse);
            let servings = item.get("servings").and_then(|s| s.as_f64()).unwrap_or(1.0);
            if let Some(i) = id.and_then(|id| self.ingredients.get(&id).ok().flatten()).filter(|i| i.owner == *owner) {
                t.add(&i, servings * multiplier);
            }
        }
        t
    }

    /// Totals eaten and calories burned in `[from, to)`.
    pub fn summary(&self, owner: &Uuid, from: u64, to: u64) -> (Totals, f64, usize) {
        let mut totals = Totals::default();
        let entries = self.eaten.find(|e| e.owner == *owner && e.ate_at >= from && e.ate_at < to).unwrap_or_default();
        for e in &entries {
            let servings = if e.servings > 0.0 { e.servings } else { 1.0 };
            if let Some(m) = e.meal_id.and_then(|id| self.meals.get(&id).ok().flatten()).filter(|m| m.owner == *owner) {
                let mt = self.meal_totals(owner, &m, servings);
                totals.kcalories += mt.kcalories;
                totals.protein += mt.protein;
                totals.carbohydrates += mt.carbohydrates;
                totals.fiber += mt.fiber;
                totals.sugar += mt.sugar;
                totals.fat += mt.fat;
                totals.saturated_fat += mt.saturated_fat;
                totals.trans_fat += mt.trans_fat;
            } else if let Some(i) = e.ingredient_id.and_then(|id| self.ingredients.get(&id).ok().flatten()).filter(|i| i.owner == *owner) {
                totals.add(&i, servings);
            }
        }
        let burned: f64 = self.workouts.find(|w| w.owner == *owner && w.performed_at >= from && w.performed_at < to).unwrap_or_default().iter().map(|w| w.kcalories_burned).sum();
        (totals, burned, entries.len())
    }
}

pub fn router(food: Food, users: Users) -> Router {
    let (f, u) = (food.clone(), users.clone());
    let summary = move |req: &Request| {
        let Some(me) = u.from_request(req) else { return Response::unauthorized() };
        let day = req.query_param("day").map(String::from).unwrap_or_else(|| unix_to_date(unix_now()));
        let Some((y, m, d)) = parse_date(&day) else { return Response::bad_request("`day` must be YYYY-MM-DD") };
        let from = date_to_unix(y, m, d);
        let (t, burned, entries) = f.summary(&me.id, from, from + 86400);
        if req.query_param("format") == Some("json") || req.header("accept").is_some_and(|a| a.contains("application/json")) {
            return Response::json(&json_obj! {"day" => day, "entries" => entries, "totals" => t.to_value(), "kcalories_burned" => burned, "net_kcalories" => t.kcalories - burned});
        }
        let rows = vec![
            vec!["Calories".to_string(), format!("{:.0}", t.kcalories)],
            vec!["Protein (g)".into(), format!("{:.1}", t.protein)],
            vec!["Carbohydrates (g)".into(), format!("{:.1}", t.carbohydrates)],
            vec!["Fat (g)".into(), format!("{:.1}", t.fat)],
            vec!["Burned".into(), format!("{burned:.0}")],
        ];
        Response::page(req, &Page::new("Food").add(heading(1, format!("Food {day}"))).add(text(format!("{entries} entries"))).add(table(["", "Total"], rows)))
    };
    Router::new()
        .get("/food/summary", summary)
        .nest("/food/ingredients", owned_crud(users.clone(), food.ingredients))
        .nest("/food/meals", owned_crud(users.clone(), food.meals))
        .nest("/food/eaten", owned_crud(users.clone(), food.eaten))
        .nest("/food/workouts", owned_crud(users, food.workouts))
        .tagged("public")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ing(owner: Uuid, name: &str, kcal: f64, protein: f64) -> Ingredient {
        Ingredient { id: Uuid::new_v4(), owner, name: name.into(), serving_size: 100.0, kcalories: kcal, protein, carbohydrates: 0.0, fiber: 0.0, sugar: 0.0, fat: 0.0, saturated_fat: 0.0, trans_fat: 0.0, created_at: 0, updated_at: 0 }
    }

    #[test]
    fn summary_math() {
        let food = Food::new(Database::memory().scope("food").unwrap()).unwrap();
        let me = Uuid::new_v4();
        let yogurt = ing(me, "Yogurt", 90.0, 18.0);
        let rice = ing(me, "Rice", 200.0, 4.0);
        let theirs = ing(Uuid::new_v4(), "Not mine", 999.0, 99.0);
        for i in [&yogurt, &rice, &theirs] {
            food.ingredients.insert(i).unwrap();
        }
        let bowl = Meal { id: Uuid::new_v4(), owner: me, name: "Bowl".into(), items: vec![json_obj! {"ingredient_id" => yogurt.id, "servings" => 2.0}, json_obj! {"ingredient_id" => rice.id}, json_obj! {"ingredient_id" => theirs.id}], created_at: 0, updated_at: 0 };
        food.meals.insert(&bowl).unwrap();
        let day = 1_700_000_000 / 86400 * 86400;
        let eat = |at: u64, meal: Option<Uuid>, ingredient: Option<Uuid>, servings: f64| {
            food.eaten.insert(&Eaten { id: Uuid::new_v4(), owner: me, ate_at: at, meal_id: meal, ingredient_id: ingredient, servings, created_at: 0, updated_at: 0 }).unwrap();
        };
        eat(day + 100, Some(bowl.id), None, 1.0); // 2*90 + 200 = 380, protein 36+4 = 40
        eat(day + 200, None, Some(yogurt.id), 0.5); // +45 kcal, +9 protein
        eat(day + 86400 + 5, None, Some(rice.id), 1.0); // next day
        food.workouts.insert(&Workout { id: Uuid::new_v4(), owner: me, name: "Run".into(), performed_at: day + 300, duration_minutes: 30.0, kcalories_burned: 250.0, notes: String::new(), created_at: 0, updated_at: 0 }).unwrap();

        let (t, burned, n) = food.summary(&me, day, day + 86400);
        assert_eq!((n, burned), (2, 250.0));
        assert_eq!(t.kcalories, 425.0);
        assert_eq!(t.protein, 49.0);
        assert_eq!(food.summary(&Uuid::new_v4(), day, day + 86400).2, 0);
    }

    #[test]
    fn summary_route() {
        let db = Database::memory();
        let users = Users::with_config(db.scope("user").unwrap(), user::UsersConfig { password_iterations: 50, ..Default::default() }).unwrap();
        users.create_user("alice", "alice password", false).unwrap();
        let cookie = format!("session_id={}", users.login("alice", "alice password").unwrap().1.id);
        let r = router(Food::new(db.scope("food").unwrap()).unwrap(), users);
        let post = |p: &str, b: &str| r.handle(Request::post(p, b).with_header("cookie", &cookie));
        let c = post("/food/ingredients/create", r#"{"name":"Oats","serving_size":40,"kcalories":150,"protein":5}"#);
        assert_eq!(c.status, 201, "{}", c.body_str());
        let iid = c.json_body().unwrap().get("content").unwrap().as_str().unwrap().to_string();
        post("/food/eaten/create", &format!(r#"{{"ate_at":1700000100,"ingredient_id":"{iid}","servings":2}}"#));
        let s = r.handle(Request::get("/food/summary?day=2023-11-14&format=json").with_header("cookie", &cookie)).json_body().unwrap();
        assert_eq!(s.path("totals.kcalories").unwrap().as_f64(), Some(300.0));
        assert_eq!(r.handle(Request::get("/food/summary?day=nope").with_header("cookie", &cookie)).status, 400);
        assert_eq!(r.handle(Request::get("/food/summary")).status, 401);
        assert!(r.handle(Request::get("/food/summary?day=2023-11-14").with_header("cookie", &cookie)).body_str().contains("Calories"));
    }
}
