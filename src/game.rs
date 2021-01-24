// Work breakdown
// - constrain the number of words generated to only as much as would fit in two panes
// - setup a better word selection algorithm which results in more common letters
// - place the words throughout the pane w/ filler text that goes in between words
// - add support for selecting between the words in the TUI and highlighting the current selection
//      - mouse support?
//      - keyboard support?
// - add support for using that selection instead of text input to power the gameloop
// - add an output pane which tells you the results of your current selection
// - refactor out tui utils into its own module
// - randomize the hex start address for fun flavor

// extensions/flavor
// - use appropriate font to give it a "fallout feel"
// - SFX

use crate::dict;
use crate::randwrapper::{select_rand, RangeRng, ThreadRangeRng};
use crate::utils::Rect;
use std::str::FromStr;

const TITLE: &str = "FONV: Terminal Cracker";

#[derive(Debug, Clone, Copy)]
pub enum Difficulty {
    VeryEasy,
    Easy,
    Average,
    Hard,
    VeryHard,
}

struct HexDumpPane {
    dump_width: i32,
    dump_height: i32,
    addr_width: i32,
    addr_to_dump_padding: i32,
}

impl HexDumpPane {
    fn height(&self) -> i32 {
        self.dump_height
    }

    fn full_width(&self) -> i32 {
        self.dump_width + self.addr_to_dump_padding + self.addr_width
    }
}

impl FromStr for Difficulty {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("VeryEasy") || s.eq_ignore_ascii_case("VE") {
            return Ok(Difficulty::VeryEasy);
        }

        if s.eq_ignore_ascii_case("Easy") || s.eq_ignore_ascii_case("E") {
            return Ok(Difficulty::Easy);
        }

        if s.eq_ignore_ascii_case("Average") || s.eq_ignore_ascii_case("A") {
            return Ok(Difficulty::Average);
        }

        if s.eq_ignore_ascii_case("Hard") || s.eq_ignore_ascii_case("H") {
            return Ok(Difficulty::Hard);
        }

        if s.eq_ignore_ascii_case("VeryHard") || s.eq_ignore_ascii_case("VH") {
            return Ok(Difficulty::VeryHard);
        }

        Err("Invalid difficulty string")
    }
}

fn generate_words(difficulty: Difficulty, rng: &mut dyn RangeRng<usize>) -> Vec<String> {
    let word_len = match difficulty {
        Difficulty::VeryEasy => 4,
        Difficulty::Easy => 6,
        Difficulty::Average => 8,
        Difficulty::Hard => 10,
        Difficulty::VeryHard => 12,
    };

    const WORDS_TO_GENERATE_COUNT: usize = 12;

    let dict_chunk = dict::EnglishDictChunk::load(word_len);
    (0..WORDS_TO_GENERATE_COUNT)
        .map(|_| dict_chunk.get_random_word(rng))
        .collect()
}

fn render_hexdump_pane(
    window: &pancurses::Window,
    hex_dump_dimensions: &HexDumpPane,
    render_rect: &Rect,
    mem_start: usize,
    bytes: &str,
) {
    for row in 0..hex_dump_dimensions.height() {
        let memaddr = mem_start + (row * hex_dump_dimensions.dump_width) as usize;
        let memaddr_str = format!(
            "0x{:0width$X}",
            memaddr,
            width = (hex_dump_dimensions.addr_width as usize) - 2,
        );

        let byte_offset = (row * hex_dump_dimensions.dump_width) as usize;
        let row_bytes = &bytes[byte_offset..][..hex_dump_dimensions.dump_width as usize];

        let y = row + render_rect.top;

        // render the memaddr
        window.mvaddstr(y, render_rect.left, &memaddr_str);

        // render the dump
        let hex_dump_bytes_offset =
            render_rect.left + memaddr_str.len() as i32 + hex_dump_dimensions.addr_to_dump_padding;
        for (i, byte) in row_bytes.chars().enumerate() {
            window.mvaddch(y, hex_dump_bytes_offset + i as i32, byte);
        }
    }
}

fn obfuscate_words(
    words: &[String],
    target_size: usize,
    rng: &mut dyn RangeRng<usize>,
) -> (String, Vec<usize>) {
    let initial_length_from_words: usize = words.iter().fold(0, |acc, word| acc + word.len());
    let remaining_char_count_to_generate = target_size - initial_length_from_words;

    // place the words at offsets within the final obfuscated string such that filling in between/around
    // those words will generate the final obfuscated string
    let offsets = {
        let mut offsets = Vec::new();
        for word in words.iter().rev() {
            offsets.push(0);
            for offset in offsets.iter_mut() {
                *offset += word.len();
            }
        }

        for _ in 0..remaining_char_count_to_generate {
            // Increment each offset starting from a random offset.
            // This simulates adding a character before a random word in the final obfuscated string
            // e.g. if offsets_to_bump_start = 1, then a character is added between words 0 and 1
            // and words 1->onward are then offset by an additional character.
            let offsets_to_bump_start = rng.gen_range(0, offsets.len() + 1);

            for i in offsets_to_bump_start..offsets.len() {
                offsets[i] += 1;
            }
        }
        offsets
    };

    let mut string_builder = String::with_capacity(target_size);

    // fill the string with the initial garbage chars
    for _ in 0..remaining_char_count_to_generate {
        let garbage_char = rng.gen_range('#' as usize, '.' as usize) as u8 as char;
        string_builder.push(garbage_char);
    }

    // insert each of the offset words
    for (word, offset) in words.iter().zip(offsets.iter()) {
        string_builder.insert_str(*offset, &word);
    }

    (string_builder, offsets)
}

pub fn run_game(difficulty: Difficulty) {
    const HEXDUMP_ROW_WIDTH: i32 = 12; // 12 characters per row of the hexdump
    const HEXDUMP_MAX_ROWS: i32 = 16; // 16 rows of hex dump per dump pane
    const HEXDUMP_MAX_BYTES: i32 = HEXDUMP_ROW_WIDTH * HEXDUMP_MAX_ROWS;
    const MEMADDR_HEX_WIDTH: i32 = "0x1234".len() as i32; // 2 byte memaddr
    const PANE_HORIZONTAL_PADDING: i32 = 4; // horizontal padding between panes in the memdump window

    const HEXDUMP_PANE_VERT_OFFSET: i32 = 5;

    // Generate a random set of words based on the difficulty
    let mut rng = ThreadRangeRng::new();
    let rand_words = generate_words(difficulty, &mut rng);
    let (obfuscated_words, word_offsets) =
        obfuscate_words(&rand_words, HEXDUMP_MAX_BYTES as usize, &mut rng);

    // For the sake of keeping the windowing around let's dump those words in a window
    {
        // setup the window
        let window = pancurses::initscr();
        pancurses::noecho(); // prevent key inputs rendering to the screen
        pancurses::cbreak();
        pancurses::curs_set(0);
        pancurses::set_title(TITLE);
        window.nodelay(true); // don't block waiting for key inputs (we'll poll)
        window.keypad(true); // let special keys be captured by the program (i.e. esc/backspace/del/arrow keys)

        // TODO just open a stub window for now. We'll write the game soon.
        window.clear();

        const HEXDUMP_ROW_WIDTH: i32 = 12; // 12 characters per row of the hexdump
        const HEXDUMP_MAX_ROWS: i32 = 16; // 16 rows of hex dump per dump pane
        const HEXDUMP_MAX_BYTES: i32 = HEXDUMP_ROW_WIDTH * HEXDUMP_MAX_ROWS;
        const MEMADDR_HEX_WIDTH: i32 = "0x1234".len() as i32; // 2 byte memaddr
        const PANE_HORIZONTAL_PADDING: i32 = 4; // horizontal padding between panes in the memdump window

        const HEXDUMP_PANE_VERT_OFFSET: i32 = 5;

        // are all of these constants only used by this struct?
        let hex_dump_pane_dimensions = HexDumpPane {
            dump_width: HEXDUMP_ROW_WIDTH,
            dump_height: HEXDUMP_MAX_ROWS,
            addr_width: MEMADDR_HEX_WIDTH,
            addr_to_dump_padding: PANE_HORIZONTAL_PADDING,
        };

        let left_hex_dump_rect = Rect {
            left: 0,
            top: HEXDUMP_PANE_VERT_OFFSET,
            width: hex_dump_pane_dimensions.full_width(),
            height: hex_dump_pane_dimensions.height(),
        };

        let right_hex_dump_rect = Rect {
            left: left_hex_dump_rect.width + hex_dump_pane_dimensions.addr_to_dump_padding,
            top: left_hex_dump_rect.top,
            width: hex_dump_pane_dimensions.full_width(),
            height: hex_dump_pane_dimensions.height(),
        };

        window.mvaddstr(0, 0, "ROBCO INDUSTRIES (TM) TERMALINK PROTOCOL");
        window.mvaddstr(1, 0, "ENTER PASSWORD NOW");
        const BLOCK_CHAR: char = '#';
        window.mvaddstr(
            3,
            0,
            format!(
                "# ATTEMPT(S) LEFT: {} {} {} {}",
                BLOCK_CHAR, BLOCK_CHAR, BLOCK_CHAR, BLOCK_CHAR
            ),
        );

        let hexdump_first_addr = 0x1234; // TODO: randomize for fun flavor
        render_hexdump_pane(
            &window,
            &hex_dump_pane_dimensions,
            &left_hex_dump_rect,
            hexdump_first_addr,
            &obfuscated_words,
        );

        render_hexdump_pane(
            &window,
            &hex_dump_pane_dimensions,
            &right_hex_dump_rect,
            hexdump_first_addr + obfuscated_words.len(),
            &obfuscated_words[obfuscated_words.len()..],
        );

        /* TODO: render the words in the memdump
        window.mvaddstr(0, 0, format!("{:?}", difficulty));
        for (i, rand_word) in rand_words.iter().enumerate() {
            window.mvaddstr(i as i32 + 1, 0, rand_word);
        }
        */

        window.refresh();
        std::thread::sleep(std::time::Duration::from_millis(5000));
        pancurses::endwin();
    }

    // now let's run a mock game_loop
    run_game_from_line_console(&rand_words, &mut rng);
}

fn run_game_from_line_console(words: &[String], rng: &mut dyn RangeRng<usize>) {
    // Select an answer
    let solution = select_rand(words, rng);

    println!("Solution: {}", solution);
    for word in words {
        let matching_char_count = crate::utils::matching_char_count_ignore_case(&solution, word);
        println!("  {} ({}/{})", word, matching_char_count, solution.len());
    }

    // On each game loop iteration...
    let mut remaining_guess_count = 4;
    while remaining_guess_count > 0 {
        // Let the user provide a guess
        println!("\nGuess? ");
        let next_guess: String = text_io::read!("{}");

        // Check for a win
        if &next_guess == solution {
            break;
        }

        // Validate the non-winning word in the guess list
        // TODO: won't be necessary when they can only select from a preset set of words
        if !words.iter().any(|w| w.eq_ignore_ascii_case(&next_guess)) {
            println!("Not a word in the list!");
            continue;
        }

        // Print the matching character count as a hint for the next guess
        let matching_char_count =
            crate::utils::matching_char_count_ignore_case(&solution, &next_guess);

        println!("{} / {} chars match!", matching_char_count, solution.len());

        // let the user know how many attempts they have left
        remaining_guess_count -= 1;
        println!("{} attempts left", remaining_guess_count);
    }

    if remaining_guess_count > 0 {
        println!("Correct!");
    } else {
        println!("Failed!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::randwrapper;

    #[test]
    fn test_word_generation() {
        let mut rng = randwrapper::mocks::SequenceRangeRng::new(&[0, 2, 4, 7]);
        let tests = [
            (Difficulty::VeryEasy, ["aahs", "aani", "abac", "abba"]),
            (Difficulty::Easy, ["aahing", "aarrgh", "abacay", "abacot"]),
            (
                Difficulty::Average,
                ["aardvark", "aaronite", "abacisci", "abacuses"],
            ),
            (
                Difficulty::Hard,
                ["aardwolves", "abalienate", "abandoning", "abaptistum"],
            ),
            (
                Difficulty::VeryHard,
                [
                    "abalienating",
                    "abandonments",
                    "abbreviately",
                    "abbreviatory",
                ],
            ),
        ];

        for (difficulty, expected_words) in &tests {
            let generated_words = generate_words(*difficulty, &mut rng);
            let expected_word_cnt = 12;
            for i in 0..expected_word_cnt {
                let generated_word = &generated_words[i];
                let expected_word = expected_words[i % expected_words.len()];
                assert_eq!(generated_word, expected_word);
            }
        }
    }

    #[test]
    fn test_obfuscate_words() {
        let mut rng = ThreadRangeRng::new();
        let words: Vec<String> = ["apple", "orange", "banana"]
            .iter()
            .map(|s| String::from(*s))
            .collect();

        const HEX_BYTE_COUNT: usize = 100;
        let (obfuscated_words, offsets) = obfuscate_words(&words, HEX_BYTE_COUNT, &mut rng);

        assert_eq!(HEX_BYTE_COUNT, obfuscated_words.len());
        for (word, word_offset) in words.iter().zip(offsets.iter()) {
            let word_in_blob = &obfuscated_words[*word_offset..][..word.len()];
            assert_eq!(word, word_in_blob);
        }
    }
}
