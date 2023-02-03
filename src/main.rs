#![allow(unused_variables, dead_code, unused_mut)]
use std::process::exit;

use chrono::Datelike;
use scraper::{Html, Selector};
use selectors::{attr::CaseSensitivity, Element};
use teloxide::{utils::markdown};

// Import week days and WeekDay

struct MealGroup {
    meal_type: String,
    sub_meals: Vec<SingleMeal>,
}

struct SingleMeal {
    name: String,
    additional_ingredients: Vec<String>,
    price: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = build_heute_msg().await;
    println!("{}", text);
    Ok(())
}



fn escape_markdown_v2(input: &str) -> String {
    let res = input
        .replace(".", r"\.")
        .replace("!", r"\!")
        .replace("+", r"\+")
        .replace("-", r"\-")
        .replace("<", r"\<")
        .replace(">", r"\>")
        .replace("(", r"\(")
        .replace(")", r"\)")
        .replace("=", r"\=")
        // workaround as '&' in html is improperly decoded
        .replace("&amp;", "&");
    res
}

async fn build_heute_msg() -> String {
    let mut msg: String = String::new();

    // get current date
    let local_date = chrono::Local::now();
    let (year, month, day) = (local_date.year(), local_date.month(), local_date.day());

    // retrieve meals
    let (v_meal_groups, ret_date) = get_meals(year, month, day).await;

    // insert date into msg
    msg += &format!("{}\n", markdown::italic(&escape_markdown_v2(&ret_date)));

    // loop over meal groups
    for meal_group in v_meal_groups {
        let mut price_is_shared = true;
        let price_first_submeal = &meal_group.sub_meals.first().unwrap().price;

        for sub_meal in &meal_group.sub_meals {
            if &sub_meal.price != price_first_submeal {
                price_is_shared = false;
                break;
            }
        }

        // Bold type of meal (-group)
        msg += &format!(
            "\n{}\n",
            markdown::bold(&escape_markdown_v2(&meal_group.meal_type))
        );

        // loop over meals in meal group
        for sub_meal in &meal_group.sub_meals {
            // underlined single or multiple meal name
            msg += &format!(
                " • {}\n",
                markdown::underline(&escape_markdown_v2(&sub_meal.name))
            );

            // loop over ingredients of meal
            for ingredient in &sub_meal.additional_ingredients {
                // appending ingredient to msg
                msg += &format!(
                    "     \\+ {}\n",
                    markdown::italic(&escape_markdown_v2(&ingredient))
                )
            }
            // appending price
            if !price_is_shared {
                msg += &format!("   {}\n", escape_markdown_v2(&sub_meal.price));
            }
        }
        if price_is_shared {
            msg += &format!("   {}\n", escape_markdown_v2(price_first_submeal));
        }
    }

    msg += &escape_markdown_v2("\n < /heute >  < /morgen >\n < /uebermorgen >");

    msg
}

async fn get_meals(year: i32, month: u32, day: u32) -> (Vec<MealGroup>, String) {
    let mut v_meal_groups: Vec<MealGroup> = Vec::new();

    // url parameters
    let loc = 140;
    let req_date_formatted: String = format!("{:04}-{:02}-{:02}", year, month, day);
    let url = format!(
        "https://www.studentenwerk-leipzig.de/mensen-cafeterien/speiseplan?location={}&date={}",
        loc, req_date_formatted
    );

    // retrieving HTML to String
    let html_text = reqwest::get(url)
        .await
        .expect("URL request failed")
        .text()
        .await
        .unwrap();

    let document = Html::parse_fragment(&html_text);

    // retrieving reported date and comparing to requested date
    let date_sel = Selector::parse(r#"select#edit-date>option[selected='selected']"#).unwrap();
    let received_date = document.select(&date_sel).next().unwrap().inner_html();

    // formatting received date to format in URL parameter,
    // to check if correct date was returned
    let received_date_formatted = format!(
        "{:04}-{:02}-{:02}",
        // year
        received_date[received_date.len() - 4..]
            .parse::<i32>()
            .unwrap(),
        // month
        received_date[received_date.len() - 7..received_date.len() - 5]
            .parse::<i32>()
            .unwrap(),
        // day
        received_date[received_date.len() - 10..received_date.len() - 8]
            .parse::<i32>()
            .unwrap(),
    );

    if received_date_formatted != req_date_formatted {
        println!("Für den Tag existiert noch kein Plan.");
        exit(0);
    }

    let container_sel = Selector::parse(r#"section.meals"#).unwrap();
    let all_child_select = Selector::parse(r#":scope > *"#).unwrap();

    let container = document.select(&container_sel).next().unwrap();

    for child in container.select(&all_child_select) {
        if child
            .value()
            .has_class("title-prim", CaseSensitivity::CaseSensitive)
        {
            // title-prim == new group -> init new group struct
            let mut meals_in_group: Vec<SingleMeal> = Vec::new();

            let mut next_sibling = child.next_sibling_element().unwrap();

            // skip headlines (or other junk elements)
            // might loop infinitely (or probably crash) if last element is not of class .accordion.ublock :)
            while !(next_sibling
                .value()
                .has_class("accordion", CaseSensitivity::CaseSensitive)
                && next_sibling
                    .value()
                    .has_class("u-block", CaseSensitivity::CaseSensitive))
            {
                next_sibling = next_sibling.next_sibling_element().unwrap();
            }

            // "next_sibling" is now of class ".accordion.u-block", aka. a group of 1 or more dishes
            // -> looping over meals in group
            for dish_element in next_sibling.select(&all_child_select) {
                let mut additional_ingredients: Vec<String> = Vec::new();

                // looping over meal ingredients
                for add_ingred_element in
                    dish_element.select(&Selector::parse(r#"details>ul>li"#).unwrap())
                {
                    additional_ingredients.push(add_ingred_element.inner_html());
                }

                // collecting into SingleMeal struct
                let meal = SingleMeal {
                    name: dish_element
                        .select(&Selector::parse(r#"header>div>div>h4"#).unwrap())
                        .next()
                        .unwrap()
                        .inner_html(),
                    additional_ingredients: additional_ingredients, //
                    price: dish_element
                        .select(&Selector::parse(r#"header>div>div>p"#).unwrap())
                        .next()
                        .unwrap()
                        .inner_html()
                        .split("\n")
                        .last()
                        .unwrap()
                        .trim()
                        .to_string(),
                };

                // pushing SingleMeal to meals struct
                meals_in_group.push(meal);
            }

            // collecting into MealGroup struct
            let meal_group = MealGroup {
                meal_type: child.inner_html(),
                sub_meals: meals_in_group,
            };

            // pushing MealGroup to MealGroups struct
            v_meal_groups.push(meal_group);
        }
    }
    (v_meal_groups, received_date)
}