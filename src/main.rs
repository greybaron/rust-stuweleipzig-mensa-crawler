use std::{env, process::exit, fs::File, io::{Write, Read}};

use chrono::{Datelike, Duration, Weekday, DateTime, Local};
use scraper::{Html, Selector};
use selectors::{attr::CaseSensitivity, Element};

#[cfg(feature = "benchmark")]
use std::time::Instant;

mod markdown {
    pub fn bold(s: &str) -> String {
        format!("*{s}*")
    }
    pub fn italic(s: &str) -> String {
        if s.starts_with("__") && s.ends_with("__") {
            format!(r"_{}\r__", &s[..s.len() - 1])
        } else {
            format!("_{s}_")
        }
    }
    pub fn underline(s: &str) -> String {
        // In case of ambiguity between italic and underline entities
        // ‘__’ is always greedily treated from left to right as beginning or end of
        // underline entity, so instead of ___italic underline___ we should use
        // ___italic underline_\r__, where \r is a character with code 13, which
        // will be ignored.
        if s.starts_with('_') && s.ends_with('_') {
            format!(r"__{s}\r__")
        } else {
            format!("__{s}__")
        }
    }
}


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
    let invalid_arg = "pass any of the following:\nheute\nmorgen\nuebermorgen\nprefetch";

    let arg: Vec<String> = env::args().collect();
    let mode: i64;
    // // mode: 0/1/2 heute/morgen/übermorgen
    // let mode: i64 = if ((arg.len() as u32) == 1) || (&arg[1] == "heute") {
    //     0
    // } else if &arg[1] == "morgen" {
    //     1
    // } else if &arg[1] == "uebermorgen" {
    //     2
    // } else {
    //     panic!("invalid option")
    // };

    if arg.len() > 1{
        match &arg[1] as &str {
            "prefetch" => {
                prefetch().await;
                exit(0)
            }
            "heute" => {
                mode = 0;
            }
            "morgen" => {
                mode = 1;
            }
            "uebermorgen" => {
                mode = 2;
            }
            _ => {
                println!("invalid argument '{}'. {}", &arg[1], invalid_arg);
                exit(1);
            }
        }
    } else {
        println!("{invalid_arg}");
        exit(2);
    }

    println!("{}", build_heute_msg(mode).await);
    Ok(())
}

async fn prefetch() {
    // only prefetching the current day, since there is no confidence at all
    // in StuWe data that is more than 2 seconds in the future 👌🏻👌🏻💯

    let now = chrono::Local::now();
    if now.weekday() == Weekday::Sat || now.weekday() == Weekday::Sun {
        exit(0);
    }
    
    let loc = 140;
    let req_date_formatted = build_req_date_string(now);

    let url_base: String = "https://www.studentenwerk-leipzig.de/mensen-cafeterien/speiseplan?".to_owned();
    let url_args = format!("location={}&date={}", loc, req_date_formatted);
    
    // getting data from server
    let html_text = reqwest::get(url_base + &url_args)
    .await
    .expect("URL request failed")
    .text()
    .await
    .unwrap();

    match File::open(&url_args) {
        // file exists, check if contents match
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents).expect("failed to read file contents");

            if contents != html_text {
                save_to_file(&url_args, &html_text);
            }
        },
        // file doesnt exist, create it
        Err(_) => {
            save_to_file(&url_args, &html_text);
        },
    }
}

fn save_to_file(url_args: &String, html_text: &String) {
    let mut file = File::create(&url_args).unwrap();
    file.write_all(html_text.as_bytes()).expect("unable to write");
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

async fn build_heute_msg(mode: i64) -> String {
    #[cfg(feature = "benchmark")]
    let now = Instant::now();

    let mut msg: String = String::new();

    // get requested date
    let mut requested_date = chrono::Local::now() + Duration::days(mode);
    let mut date_raised_by_days = 0;

    match requested_date.weekday() {
        // sat -> change req_date to mon
        Weekday::Sat => {
            requested_date += Duration::days(2);
            date_raised_by_days = 2;
        }
        Weekday::Sun => {
            requested_date += Duration::days(1);
            date_raised_by_days = 1;
        }
        _ => {
            // Any other weekday is fine, nothing to do
        }
    }

    #[cfg(feature = "benchmark")]
    println!("req setup took: {:.2?}", now.elapsed());

    // retrieve meals
    let (v_meal_groups, ret_date) = get_meals(requested_date).await;
    
    // start message formatting
    #[cfg(feature = "benchmark")]
    let now = Instant::now();

    // if mode=0, then "today" was requested. So if date_raised_by_days is != 0 AND mode=0, append warning
    let future_day_info = if mode == 0 && date_raised_by_days == 1 {
        "(Morgen)"
    } else if mode == 0 && date_raised_by_days == 2 {
        "(Übermorgen)"
    } else {
        ""
    };

    // insert date+future day info into msg
    msg += &format!(
        "{} {}\n",
        markdown::italic(&ret_date),
        future_day_info
    );

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
            markdown::bold(&meal_group.meal_type)
        );

        // loop over meals in meal group
        for sub_meal in &meal_group.sub_meals {
            // underlined single or multiple meal name
            msg += &format!(
                " • {}\n",
                markdown::underline(&sub_meal.name)
            );

            // loop over ingredients of meal
            for ingredient in &sub_meal.additional_ingredients {
                // appending ingredient to msg
                msg += &format!(
                    "     + {}\n",
                    markdown::italic(&ingredient)
                )
            }
            // appending price
            if !price_is_shared {
                msg += &format!("   {}\n", sub_meal.price);
            }
        }
        if price_is_shared {
            msg += &format!("   {}\n", price_first_submeal);
        }
    }

    msg += "\n < /heute >  < /morgen >\n < /uebermorgen >";
    
    #[cfg(feature = "benchmark")]
    println!("message build took: {:.2?}\n\n", now.elapsed());

    // return
    escape_markdown_v2(&msg)
}


fn build_req_date_string(requested_date: DateTime<Local>) -> String {
    let (year, month, day) = (
        requested_date.year(),
        requested_date.month(),
        requested_date.day(),
    );

    let out: String = format!("{:04}-{:02}-{:02}", year, month, day);
    out
}


async fn get_meals(requested_date: DateTime<Local>) -> (Vec<MealGroup>, String) {
    #[cfg(feature = "benchmark")]
    let now = Instant::now();
    let mut v_meal_groups: Vec<MealGroup> = Vec::new();

    // url parameters
    let loc = 140;
    let req_date_formatted = build_req_date_string(requested_date);
    let url_base = "https://www.studentenwerk-leipzig.de/mensen-cafeterien/speiseplan?";
    let url_params = format!("location={}&date={}", loc, req_date_formatted);

    let mut html_text: String = String::new();

    match File::open(&url_params) {
        // cached file exists, use that
        Ok(mut file) => {
            file.read_to_string(&mut html_text).expect("failed to read file contents");
        },
        // no cached file, use reqwest
        Err(_) => {
            // retrieving HTML to String
            html_text = reqwest::get(url_base.to_string() + &url_params)
            .await
            .expect("URL request failed")
            .text()
            .await
            .unwrap();
        }
    }

    
    
    #[cfg(feature = "benchmark")]
    println!("req return took: {:.2?}", now.elapsed());

    #[cfg(feature = "benchmark")]
    let now = Instant::now();
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
    #[cfg(feature = "benchmark")]
    println!("parsing took: {:.2?}", now.elapsed());
    (v_meal_groups, received_date)
}
