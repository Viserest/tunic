/// Individual ingredients
///
/// Example:
/// - Yogurt (1 cup): 90 kcalories, 18g protein, 8g carbs, ...
/// - Ground Beef (1lb): ...
/// - Rice (1 cup cooked): ...
struct Ingredient {
    // Name can be the seperator between cooked and uncooked ingredient nutrition
    name: String,
    serving_size: u16,
    kcalories: u16,
    protein: u16,
    carbohydrates: u16,
    fiber: u16,
    sugar: u16,
    fat: u16,
    saturated_fat: u16,
    trans_fat: u16,
}

/// Named meals are a collection of [`Ingredient`]s
struct Meal {
    name: String,
    ingredients: Vec<Ingredient>,
}

/// Ate Meals are a collection of [`Meal`]s and/or [`Ingredient`]s ate at a certain time
struct AteMeal {
    ate_at: u64,
    meal: Vec<Meal>,
    ingredients: Vec<Ingredient>,
    // TIMESTAMPS
    created_at: u64,
    accessed_at: u64,
    updated_at: u64,
    deleted_at: u64,
}

/// Days are a collection of [`AteMeal`]s
struct Day {
    ate_meals: Vec<AteMeal>,
}

/// Log is a collection of [`Day`]s and [`Meal`]s \
/// Meals here can be easily used in [`AteMeal`] by cloning
struct Log {
    // Log meals to use
    meals: Vec<Meal>,
    // Daily log of ate meals
    days: Vec<Day>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
