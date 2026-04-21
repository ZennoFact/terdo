use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const TASKS_TITLE: &str = "TODO List";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RgbColor {
    r: u8,
    g: u8,
    b: u8,
}

impl RgbColor {
    fn to_crossterm_color(&self) -> Color {
        Color::Rgb { r: self.r, g: self.g, b: self.b }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct ColorScheme {
    selected_bg: RgbColor,
    selected_fg: RgbColor,
    inactive_selected_bg: RgbColor,
    inactive_selected_fg: RgbColor,
    delete_bg: RgbColor,
    title_fg: RgbColor,
    filter_all_fg: RgbColor,
    filter_completed_fg: RgbColor,
    filter_unfinished_fg: RgbColor,
    delete_fg: RgbColor,
    empty_view_fg: RgbColor,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            selected_bg: RgbColor { r: 173, g: 216, b: 230 },
            selected_fg: RgbColor { r: 40, g: 40, b: 40 },
            inactive_selected_bg: RgbColor { r: 169, g: 169, b: 169 },
            inactive_selected_fg: RgbColor { r: 0, g: 0, b: 0 },
            delete_bg: RgbColor { r: 180, g: 80, b: 80 },
            title_fg: RgbColor { r: 80, g: 184, b: 255 },
            filter_all_fg: RgbColor { r: 173, g: 216, b: 230 },
            filter_completed_fg: RgbColor { r: 144, g: 238, b: 144 },
            filter_unfinished_fg: RgbColor { r: 255, g: 255, b: 153 },
            delete_fg: RgbColor { r: 178, g: 34, b: 34 },
            empty_view_fg: RgbColor { r: 160, g: 240, b: 160 },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    colors: ColorScheme,
    split_view: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            colors: ColorScheme::default(),
            split_view: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    id: String,
    title: String,
    completed: bool,
    parent_id: Option<String>,
}

impl Task {
    fn new(title: String, parent_id: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title,
            completed: false,
            parent_id,
        }
    }
}

struct App {
    tasks: Vec<Task>,
    selected_index: usize,
    right_pane_selected_index: usize,
    current_parent: Option<String>,
    input_mode: InputMode,
    input_buffer: String,
    editing_task_id: Option<String>,
    deleting_task_id: Option<String>,
    filter_mode: FilterMode,
    split_view: bool,
    active_pane: Pane,
    settings: Settings,
    tasks_path: PathBuf,
    settings_path: PathBuf,
}

#[derive(PartialEq)]
enum InputMode {
    Normal,
    Adding,
    Editing,
    Deleting,
    Help,
}

#[derive(PartialEq, Clone, Copy)]
enum FilterMode {
    Unfinished,
    Completed,
    All,
}

#[derive(PartialEq)]
enum Pane {
    Left,
    Right,
}

impl App {
    fn new() -> io::Result<Self> {
        let (tasks_path, settings_path, settings) = initialize_config_dir()?;
        let tasks = load_tasks(&tasks_path)?;
        let split_view = settings.split_view;
        Ok(Self {
            tasks,
            selected_index: 0,
            right_pane_selected_index: 0,
            current_parent: None,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            editing_task_id: None,
            deleting_task_id: None,
            filter_mode: FilterMode::Unfinished,
            split_view,
            active_pane: Pane::Left,
            settings,
            tasks_path,
            settings_path,
        })
    }

    fn get_current_tasks(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.parent_id == self.current_parent)
            .filter(|t| match self.filter_mode {
                FilterMode::Unfinished => !t.completed,
                FilterMode::Completed => {
                    // 完了済みタスク、または完了済みサブタスクを持つ未完了タスクを表示
                    t.completed || (!t.completed && self.has_completed_subtasks(&t.id))
                },
                FilterMode::All => true,
            })
            .collect()
    }

    fn get_subtasks(&self, parent_id: &str) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.parent_id.as_deref() == Some(parent_id))
            .collect()
    }

    fn has_completed_subtasks(&self, parent_id: &str) -> bool {
        self.tasks
            .iter()
            .any(|t| t.parent_id.as_deref() == Some(parent_id) && t.completed)
    }

    fn get_filtered_subtasks(&self, parent_id: &str) -> Vec<&Task> {
        let all_subtasks = self.get_subtasks(parent_id);
        all_subtasks.into_iter().filter(|t| match self.filter_mode {
            FilterMode::Unfinished => !t.completed,
            FilterMode::Completed => t.completed,
            FilterMode::All => true,
        }).collect()
    }

    fn get_parent_for_new_task(&self) -> Option<String> {
        if self.split_view && self.active_pane == Pane::Right {
            // 右ペインがアクティブな場合、左ペインで選択されている親タスクのIDを返す
            let parent_tasks = self.get_current_tasks();
            if !parent_tasks.is_empty() && self.selected_index < parent_tasks.len() {
                Some(parent_tasks[self.selected_index].id.clone())
            } else {
                None
            }
        } else {
            // 左ペインまたは非分割時は current_parent を返す
            self.current_parent.clone()
        }
    }

    fn add_task(&mut self) {
        if !self.input_buffer.trim().is_empty() {
            let parent_id = self.get_parent_for_new_task();
            let task = Task::new(self.input_buffer.clone(), parent_id);
            self.tasks.push(task);
            let _ = save_tasks(&self.tasks, &self.tasks_path);
        }
        self.input_buffer.clear();
        self.input_mode = InputMode::Normal;
    }

    fn edit_task(&mut self) {
        if let Some(task_id) = &self.editing_task_id {
            if !self.input_buffer.trim().is_empty() {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == *task_id) {
                    task.title = self.input_buffer.clone();
                    let _ = save_tasks(&self.tasks, &self.tasks_path);
                }
            }
        }
        self.input_buffer.clear();
        self.editing_task_id = None;
        self.input_mode = InputMode::Normal;
    }

    fn start_editing(&mut self) {
        if self.split_view && self.active_pane == Pane::Right {
            let parent_tasks = self.get_current_tasks();
            if !parent_tasks.is_empty() && self.selected_index < parent_tasks.len() {
                let selected_task_id = &parent_tasks[self.selected_index].id;
                let subtasks = self.get_filtered_subtasks(selected_task_id);
                if !subtasks.is_empty() && self.right_pane_selected_index < subtasks.len() {
                    let task_id = subtasks[self.right_pane_selected_index].id.clone();
                    let task_title = subtasks[self.right_pane_selected_index].title.clone();
                    
                    self.editing_task_id = Some(task_id);
                    self.input_buffer = task_title;
                    self.input_mode = InputMode::Editing;
                }
            }
        } else {
            let current_tasks = self.get_current_tasks();
            if !current_tasks.is_empty() && self.selected_index < current_tasks.len() {
                let task_id = current_tasks[self.selected_index].id.clone();
                let task_title = current_tasks[self.selected_index].title.clone();
                drop(current_tasks); // 参照を解放
                
                self.editing_task_id = Some(task_id);
                self.input_buffer = task_title;
                self.input_mode = InputMode::Editing;
            }
        }
    }

    fn start_deleting(&mut self) {
        if self.split_view && self.active_pane == Pane::Right {
            let parent_tasks = self.get_current_tasks();
            if !parent_tasks.is_empty() && self.selected_index < parent_tasks.len() {
                let selected_task_id = &parent_tasks[self.selected_index].id;
                let subtasks = self.get_filtered_subtasks(selected_task_id);
                if !subtasks.is_empty() && self.right_pane_selected_index < subtasks.len() {
                    let task_id = subtasks[self.right_pane_selected_index].id.clone();
                    
                    self.deleting_task_id = Some(task_id);
                    self.input_mode = InputMode::Deleting;
                }
            }
        } else {
            let current_tasks = self.get_current_tasks();
            if !current_tasks.is_empty() && self.selected_index < current_tasks.len() {
                let task_id = current_tasks[self.selected_index].id.clone();
                drop(current_tasks); // 参照を解放
                
                self.deleting_task_id = Some(task_id);
                self.input_mode = InputMode::Deleting;
            }
        }
    }

    fn confirm_delete(&mut self) {
        if let Some(task_id) = &self.deleting_task_id {
            self.tasks.retain(|t| t.id != *task_id && t.parent_id.as_deref() != Some(task_id));
            let _ = save_tasks(&self.tasks, &self.tasks_path);
            
            if self.split_view && self.active_pane == Pane::Right {
                let parent_tasks = self.get_current_tasks();
                if !parent_tasks.is_empty() && self.selected_index < parent_tasks.len() {
                    let selected_task_id = &parent_tasks[self.selected_index].id;
                    let subtasks = self.get_filtered_subtasks(selected_task_id);
                    if self.right_pane_selected_index >= subtasks.len() && self.right_pane_selected_index > 0 {
                        self.right_pane_selected_index -= 1;
                    }
                }
            } else {
                if self.selected_index >= self.get_current_tasks().len() && self.selected_index > 0 {
                    self.selected_index -= 1;
                }
            }
        }
        self.deleting_task_id = None;
        self.input_mode = InputMode::Normal;
    }

    fn cancel_delete(&mut self) {
        self.deleting_task_id = None;
        self.input_mode = InputMode::Normal;
    }

    fn delete_task(&mut self) {
        self.start_deleting();
    }

    fn toggle_complete(&mut self) {
        if self.split_view && self.active_pane == Pane::Right {
            let parent_tasks = self.get_current_tasks();
            if !parent_tasks.is_empty() && self.selected_index < parent_tasks.len() {
                let selected_task_id = &parent_tasks[self.selected_index].id;
                let subtasks = self.get_filtered_subtasks(selected_task_id);
                if !subtasks.is_empty() && self.right_pane_selected_index < subtasks.len() {
                    let task_id = subtasks[self.right_pane_selected_index].id.clone();
                    if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                        task.completed = !task.completed;
                        let _ = save_tasks(&self.tasks, &self.tasks_path);
                    }
                }
            }
        } else {
            let current_tasks = self.get_current_tasks();
            if !current_tasks.is_empty() && self.selected_index < current_tasks.len() {
                let task_id = current_tasks[self.selected_index].id.clone();
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
                    task.completed = !task.completed;
                    let _ = save_tasks(&self.tasks, &self.tasks_path);
                }
            }
        }
    }

    fn move_up(&mut self) {
        if self.split_view && self.active_pane == Pane::Right {
            if self.right_pane_selected_index > 0 {
                self.right_pane_selected_index -= 1;
            }
        } else {
            if self.selected_index > 0 {
                self.selected_index -= 1;
                if self.split_view {
                    self.right_pane_selected_index = 0;
                }
            }
        }
    }

    fn move_down(&mut self) {
        if self.split_view && self.active_pane == Pane::Right {
            let parent_tasks = self.get_current_tasks();
            if !parent_tasks.is_empty() && self.selected_index < parent_tasks.len() {
                let selected_task_id = &parent_tasks[self.selected_index].id;
                let subtasks = self.get_filtered_subtasks(selected_task_id);
                if self.right_pane_selected_index < subtasks.len().saturating_sub(1) {
                    self.right_pane_selected_index += 1;
                }
            }
        } else {
            let current_tasks = self.get_current_tasks();
            if self.selected_index < current_tasks.len().saturating_sub(1) {
                self.selected_index += 1;
                if self.split_view {
                    self.right_pane_selected_index = 0;
                }
            }
        }
    }

    fn enter_subtask(&mut self) {
        let current_tasks = self.get_current_tasks();
        if !current_tasks.is_empty() && self.selected_index < current_tasks.len() {
            if self.split_view {
                self.active_pane = Pane::Right;
                self.right_pane_selected_index = 0;
            } else {
                let task_id = current_tasks[self.selected_index].id.clone();
                self.current_parent = Some(task_id);
                self.selected_index = 0;
            }
        }
    }

    fn back_to_parent(&mut self) {
        if self.split_view {
            self.active_pane = Pane::Left;
        } else if let Some(current_parent_id) = &self.current_parent {
            if let Some(parent_task) = self.tasks.iter().find(|t| t.id == *current_parent_id) {
                self.current_parent = parent_task.parent_id.clone();
                self.selected_index = 0;
            } else {
                self.current_parent = None;
                self.selected_index = 0;
            }
        }
    }

    // フィルター表示用のテキストと色を取得
    fn get_filter_display(&self) -> (&str, Color) {
        match self.filter_mode {
            FilterMode::Unfinished => ("unfinished", self.settings.colors.filter_unfinished_fg.to_crossterm_color()),
            FilterMode::Completed => ("completed", self.settings.colors.filter_completed_fg.to_crossterm_color()),
            FilterMode::All => ("all task", self.settings.colors.filter_all_fg.to_crossterm_color()),
        }
    }
}

// 空のビュー表示用の色を取得
fn get_empty_view_color(is_active: bool, colors: &ColorScheme) -> Color {
    if is_active {
        colors.empty_view_fg.to_crossterm_color()
    } else {
        Color::DarkGrey
    }
}

fn initialize_config_dir() -> io::Result<(PathBuf, PathBuf, Settings)> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory not found"))?;
    
    let config_dir = home_dir.join(".config");
    let terdo_dir = config_dir.join("terdo");
    let settings_path = terdo_dir.join("setting.toml");
    let tasks_path = terdo_dir.join("tasks.csv");
    
    // .config ディレクトリを作成
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }
    
    // terdo ディレクトリを作成
    if !terdo_dir.exists() {
        fs::create_dir_all(&terdo_dir)?;
    }
    
    // 設定ファイルの読み込みまたは作成
    let settings = if settings_path.exists() {
        let content = fs::read_to_string(&settings_path)?;
        let loaded_settings: Settings = toml::from_str(&content).unwrap_or_else(|_| Settings::default());
        
        // 設定ファイルを最新の形式で上書き（新しいフィールドを追加）
        let toml_string = toml::to_string_pretty(&loaded_settings)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(&settings_path, toml_string)?;
        
        loaded_settings
    } else {
        let default_settings = Settings::default();
        let toml_string = toml::to_string_pretty(&default_settings)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        fs::write(&settings_path, toml_string)?;
        default_settings
    };
    
    // tasks.csv を作成（存在しない場合）
    if !tasks_path.exists() {
        fs::write(&tasks_path, "")?;
    }
    
    Ok((tasks_path, settings_path, settings))
}

fn load_tasks(path: &PathBuf) -> io::Result<Vec<Task>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let mut reader = csv::Reader::from_path(path)?;
    let mut tasks = Vec::new();
    
    for result in reader.deserialize() {
        let task: Task = result?;
        tasks.push(task);
    }
    
    Ok(tasks)
}

fn save_tasks(tasks: &[Task], path: &PathBuf) -> io::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    
    for task in tasks {
        writer.serialize(task)?;
    }
    
    writer.flush()?;
    Ok(())
}

fn save_settings(settings: &Settings, path: &PathBuf) -> io::Result<()> {
    let toml_string = toml::to_string_pretty(settings)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    fs::write(path, toml_string)?;
    Ok(())
}

fn main() -> io::Result<()> {
    let mut app = App::new()?;
    
    // ターミナルセットアップ
    terminal::enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    
    let result = run_app(&mut app, &mut stdout);
    
    // クリーンアップ
    execute!(stdout, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    
    result
}

fn run_app<W: Write>(app: &mut App, stdout: &mut W) -> io::Result<()> {
    loop {
        draw(app, stdout)?;
        
        if let Event::Key(key) = event::read()? {
            // Pressイベントのみを処理（ReleaseやRepeatは無視）
            if key.kind == KeyEventKind::Press {
                if app.input_mode == InputMode::Adding || app.input_mode == InputMode::Editing || app.input_mode == InputMode::Deleting {
                    handle_input_mode(app, key);
                } else {
                    if handle_normal_mode(app, key)? {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

fn handle_input_mode(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            if app.input_mode == InputMode::Adding {
                app.add_task();
            } else if app.input_mode == InputMode::Editing {
                app.edit_task();
            }
        }
        KeyCode::Esc => {
            app.input_buffer.clear();
            app.editing_task_id = None;
            app.deleting_task_id = None;
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Char('m') => {
            // ヘルプモードの場合は終了
            if app.input_mode == InputMode::Help {
                app.input_mode = InputMode::Normal;
            }
        }
        KeyCode::Char(c @ ('y' | 'Y')) => {
            if app.input_mode == InputMode::Deleting {
                app.confirm_delete();
            } else {
                app.input_buffer.push(c);
            }
        }
        KeyCode::Char(c @ ('n' | 'N')) => {
            if app.input_mode == InputMode::Deleting {
                app.cancel_delete();
            } else {
                app.input_buffer.push(c);
            }
        }
        KeyCode::Char(c) => {
            if app.input_mode != InputMode::Deleting && app.input_mode != InputMode::Help {
                app.input_buffer.push(c);
            } else if app.input_mode == InputMode::Deleting {
                // 削除モードでy/n以外の文字が入力されたらキャンセル
                app.cancel_delete();
            }
        }
        KeyCode::Backspace => {
            if app.input_mode != InputMode::Deleting && app.input_mode != InputMode::Help {
                app.input_buffer.pop();
            }
        }
        _ => {
            if app.input_mode == InputMode::Deleting {
                // その他のキーでもキャンセル
                app.cancel_delete();
            } else if app.input_mode == InputMode::Help {
                // ヘルプモード時はその他のキーで終了
                app.input_mode = InputMode::Normal;
            }
        }
    }
}

fn handle_normal_mode(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('a') => {
            app.filter_mode = FilterMode::All;
            app.selected_index = 0;
        }
        KeyCode::Char('u') => {
            app.filter_mode = FilterMode::Unfinished;
            app.selected_index = 0;
        }
        KeyCode::Char('c') => {
            app.filter_mode = FilterMode::Completed;
            app.selected_index = 0;
        }
        KeyCode::Char('n') => {
            app.input_mode = InputMode::Adding;
        }
        KeyCode::Char('e') => {
            app.start_editing();
        }
        KeyCode::Char('d') => {
            app.delete_task();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.move_up();
        }
        KeyCode::Char(' ') => {
            app.toggle_complete();
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            app.enter_subtask();
        }
        KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
            app.back_to_parent();
        }
        KeyCode::Char('|') => {
            app.split_view = !app.split_view;
            app.settings.split_view = app.split_view;
            let _ = save_settings(&app.settings, &app.settings_path);
        }
        KeyCode::Char('m') => {
            app.input_mode = InputMode::Help;
        }
        _ => {}
    }
    Ok(false)
}

// テキストを指定幅で折り返す（表示幅を考慮）
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;
    
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(0);
        
        if current_width + char_width > max_width && !current_line.is_empty() {
            // 現在の行を保存して新しい行を開始
            lines.push(current_line.clone());
            current_line.clear();
            current_width = 0;
        }
        
        current_line.push(ch);
        current_width += char_width;
    }
    
    // 最後の行を追加
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    
    if lines.is_empty() {
        lines.push(String::new());
    }
    
    lines
}

fn draw<W: Write>(app: &App, stdout: &mut W) -> io::Result<()> {
    execute!(stdout, terminal::Clear(ClearType::All))?;
    
    let (width, height) = terminal::size()?;
    let content_height = height.saturating_sub(2);
    
    if app.input_mode == InputMode::Help && !app.split_view {
        // ヘルプモードかつ非分割時：全画面でヘルプを表示
        draw_help_full(app, stdout, width, height)?;
    } else if app.input_mode == InputMode::Help && app.split_view {
        // ヘルプモードかつ分割時：アクティブペインにタスク、非アクティブペインにヘルプ
        if app.active_pane == Pane::Left {
            draw_parent_tasks(app, stdout, width / 2, content_height)?;
            draw_split_divider(stdout, width / 2, content_height, &app.settings.colors)?;
            draw_help_pane(app, stdout, width, content_height)?;
        } else {
            draw_help_pane(app, stdout, width / 2, content_height)?;
            draw_split_divider(stdout, width / 2, content_height, &app.settings.colors)?;
            draw_subtasks(app, stdout, width, content_height)?;
        }
    } else {
        // 通常モード
        draw_parent_tasks(app, stdout, if app.split_view { width / 2 } else { width }, content_height)?;
        
        if app.split_view {
            draw_split_divider(stdout, width / 2, content_height, &app.settings.colors)?;
            draw_subtasks(app, stdout, width, content_height)?;
        }
    }
    
    // ヘルプまたは入力エリアを描画
    draw_bottom_area(app, stdout, height)?;
    
    stdout.flush()?;
    Ok(())
}

fn draw_parent_tasks<W: Write>(app: &App, stdout: &mut W, width: u16, height: u16) -> io::Result<()> {
    let current_tasks = app.get_current_tasks();
    let colors = &app.settings.colors;
    let is_split_and_active_left = app.split_view && app.active_pane == Pane::Left;
    let is_split_and_active_right = app.split_view && app.active_pane == Pane::Right;
    
    // タイトル
    queue!(stdout, cursor::MoveTo(0, 0))?;
    if let Some(parent_id) = &app.current_parent {
        if let Some(parent) = app.tasks.iter().find(|t| t.id == *parent_id) {
            queue!(stdout, SetForegroundColor(colors.title_fg.to_crossterm_color()), Print(format!("Subtasks of: {}", parent.title)), ResetColor)?;
        }
    } else {
        queue!(stdout, SetForegroundColor(colors.title_fg.to_crossterm_color()), Print(TASKS_TITLE), ResetColor)?;
    }
    
    // フィルター状態を右端に表示
    let (filter_text, filter_color) = app.get_filter_display();
    let filter_pos = width.saturating_sub(filter_text.len() as u16 + if app.split_view { 2 } else { 1 });
    queue!(stdout, cursor::MoveTo(filter_pos, 0), SetForegroundColor(filter_color), Print(filter_text), ResetColor)?;
    
    // タスクリスト
    if current_tasks.is_empty() && app.current_parent.is_some() {
        // サブタスクビューで空の場合
        let is_active = !(app.split_view && !is_split_and_active_left);
        let fg_color = get_empty_view_color(is_active, colors);
        queue!(stdout, 
            cursor::MoveTo(0, 1),
            SetForegroundColor(fg_color),
            Print("create (n)ew sub-task"),
            ResetColor
        )?;
    } else if current_tasks.is_empty() && app.current_parent.is_none() {
        // 親タスクが空の場合
        let is_active = !(app.split_view && !is_split_and_active_left);
        let fg_color = get_empty_view_color(is_active, colors);
        queue!(stdout, 
            cursor::MoveTo(0, 1),
            SetForegroundColor(fg_color),
            Print("create (n)ew task"),
            ResetColor
        )?;
    } else {
        let mut current_line = 0u16;  // 現在の描画行
        
        for (i, task) in current_tasks.iter().enumerate() {
            if current_line >= height {
                break;  // 画面に収まらない場合は終了
            }
            
            let selected = i == app.selected_index;
            let prefix = if selected { "> " } else { "  " };
            let status = if task.completed { "[x]" } else { "[ ]" };
            let has_subtasks = !app.get_subtasks(&task.id).is_empty();
            let subtask_indicator = if has_subtasks && !app.split_view { " +" } else { "" };
            
            // completedフィルター時に親タスクに完了済みサブタスクがあるかチェック
            let is_parent_with_completed_subtasks = 
                app.filter_mode == FilterMode::Completed && 
                !task.completed && 
                app.has_completed_subtasks(&task.id);
            
            let display = format!("{}{} {}{}", prefix, status, task.title, subtask_indicator);
            
            // 折り返し処理
            let max_width = if app.split_view { 
                (width as usize).saturating_sub(2) 
            } else { 
                width as usize 
            };
            
            let wrapped_lines = if display.width() > max_width {
                wrap_text(&display, max_width)
            } else {
                vec![display.clone()]
            };
            
            // 各行を描画
            for (line_idx, line) in wrapped_lines.iter().enumerate() {
                if current_line >= height {
                    break;
                }
                
                queue!(stdout, cursor::MoveTo(0, current_line + 1))?;
                
                let is_first_line = line_idx == 0;
                let display_text = if is_first_line {
                    line.clone()
                } else {
                    // 2行目以降はインデント
                    format!("    {}", line)
                };
                
                if selected && !is_split_and_active_right {
                    let padding = if app.split_view {
                        " ".repeat(max_width.saturating_sub(display_text.width()))
                    } else {
                        " ".repeat((width as usize).saturating_sub(display_text.width()))
                    };
                    queue!(stdout, 
                        SetBackgroundColor(colors.selected_bg.to_crossterm_color()),
                        SetForegroundColor(colors.selected_fg.to_crossterm_color()),
                        Print(format!("{}{}", display_text, padding)),
                        ResetColor
                    )?;
                } else if selected && is_split_and_active_right {
                    let padding = " ".repeat(max_width.saturating_sub(display_text.width()));
                    queue!(stdout, 
                        SetBackgroundColor(colors.inactive_selected_bg.to_crossterm_color()),
                        SetForegroundColor(colors.inactive_selected_fg.to_crossterm_color()),
                        Print(format!("{}{}", display_text, padding)),
                        ResetColor
                    )?;
                } else if is_parent_with_completed_subtasks {
                    // 完了済みサブタスクを持つ未完了の親タスクはグレー
                    queue!(stdout, 
                        SetForegroundColor(Color::DarkGrey),
                        Print(&display_text),
                        ResetColor
                    )?;
                } else {
                    // 通常表示（白色）
                    queue!(stdout, 
                        SetForegroundColor(Color::White),
                        Print(&display_text),
                        ResetColor
                    )?;
                }
                
                current_line += 1;
            }
        }
    }
    
    Ok(())
}

fn draw_split_divider<W: Write>(stdout: &mut W, split_x: u16, height: u16, colors: &ColorScheme) -> io::Result<()> {
    for y in 0..=height {
        queue!(stdout, 
            cursor::MoveTo(split_x, y), 
            SetForegroundColor(colors.inactive_selected_bg.to_crossterm_color()),
            Print("│"),
            ResetColor
        )?;
    }
    Ok(())
}

fn draw_help_full<W: Write>(app: &App, stdout: &mut W, _width: u16, height: u16) -> io::Result<()> {
    let colors = &app.settings.colors;
    
    // タイトル
    queue!(stdout, 
        cursor::MoveTo(0, 0),
        SetForegroundColor(colors.title_fg.to_crossterm_color()),
        Print("Manual"),
        ResetColor
    )?;
    
    // ヘルプ内容（プレースホルダー）
    let help_lines = vec![
        "",
        "Manual: TerDO の使い方",
        "",
        "キー - 対応する動作",
        "  A - 全てのタスクを表示",
        "  C - 完了済みタスクを表示",
        "  D - タスクの削除",
        "  E - タスクの編集",
        "  F - タスクの検索（未実装）",
        "  H, ←, BACKSPACE - 親タスクに戻る",
        "  J, ↓ - タスクを下に移動",
        "  K, ↑ - タスクを上に移動",
        "  L, →, ENTER - サブタスクに移動",
        "  M - ヘルプを表示・非表示",
        "  N - タスクの作成",
        "  Q - アプリケーションを終了",
        "  R - タスクのリストを更新",
        "  S - タスクの並び替え",
        "  T - タスクの完了状態を切り替え",
        "  | - 画面分割の切り替えON/OFF",
        "  SPACE - タスクの完了状態を切り替え",
        "",
    ];
    
    for (i, line) in help_lines.iter().enumerate() {
        if i + 1 >= height as usize {
            break;
        }
        queue!(stdout,
            cursor::MoveTo(2, (i + 1) as u16),
            Print(line)
        )?;
    }
    
    Ok(())
}

fn draw_help_pane<W: Write>(app: &App, stdout: &mut W, width: u16, height: u16) -> io::Result<()> {
    let colors = &app.settings.colors;
    let split_x = if app.active_pane == Pane::Left { width / 2 } else { 0 };
    let pane_width = width / 2;
    
    // タイトル
    queue!(stdout, 
        cursor::MoveTo(split_x + 1, 0),
        SetForegroundColor(colors.title_fg.to_crossterm_color()),
        Print("Manual"),
        ResetColor
    )?;
    
    // ヘルプ内容（プレースホルダー）
    let help_lines = vec![
        "",
        "Manual: TerDO の使い方",
        "",
        "キー - 対応する動作",
        "  A - all: 全てのタスクを表示",
        "  C - completed: 完了済みタスクを表示",
        "  D - delete: タスクの削除",
        "  E - edit: タスクの編集",
        "  F - find: タスクの検索（未実装）",
        "  H, ←, BACKSPACE - back/left: 親タスクに戻る / 左ペインをアクティブ",
        "  J, ↓ - down: タスクを下に移動",
        "  K, ↑ - up: タスクを上に移動",
        "  L, →, ENTER - right: サブタスクに移動 / 右ペインをアクティブ",
        "  M - manual: ヘルプを表示・非表示",
        "  N - new: タスクの作成",
        "  Q - quit: アプリケーションを終了",
        "  R - refresh: タスクのリストを更新",
        "  S - sort: タスクの並び替え",
        "  T - toggle: タスクの完了状態を切り替え",
        "  | - split: 画面分割の切り替えON/OFF",
        "  SPACE - toggle complete: タスクの完了状態を切り替え",
        "",
    ];
    
    for (i, line) in help_lines.iter().enumerate() {
        if i + 1 >= height as usize {
            break;
        }
        
        // ペイン幅を考慮して切り詰め
        let max_width = pane_width.saturating_sub(3) as usize;
        let truncated = if line.width() > max_width {
            let mut current_width = 0;
            let mut truncated_line = String::new();
            
            for ch in line.chars() {
                let char_width = ch.width().unwrap_or(0);
                if current_width + char_width > max_width {
                    break;
                }
                truncated_line.push(ch);
                current_width += char_width;
            }
            truncated_line
        } else {
            line.to_string()
        };
        
        queue!(stdout,
            cursor::MoveTo(split_x + 2, (i + 1) as u16),
            Print(&truncated)
        )?;
    }
    
    Ok(())
}

fn draw_subtasks<W: Write>(app: &App, stdout: &mut W, width: u16, height: u16) -> io::Result<()> {
    let split_x = width / 2;
    let parent_tasks = app.get_current_tasks();
    let colors = &app.settings.colors;
    
    // 右ペイン（サブタスク）
    queue!(stdout, cursor::MoveTo(split_x + 1, 0), SetForegroundColor(colors.title_fg.to_crossterm_color()), Print("Subtasks"), ResetColor)?;
    
    // フィルター状態を右ペインの右端に表示
    let (filter_text, filter_color) = app.get_filter_display();
    let right_pane_width = width - split_x - 1;
    let filter_pos = split_x + 1 + right_pane_width.saturating_sub(filter_text.len() as u16 + 1);
    queue!(stdout, cursor::MoveTo(filter_pos, 0), SetForegroundColor(filter_color), Print(filter_text), ResetColor)?;
    
    if !parent_tasks.is_empty() && app.selected_index < parent_tasks.len() {
        let selected_task_id = &parent_tasks[app.selected_index].id;
        let subtasks = app.get_filtered_subtasks(selected_task_id);
        
        if subtasks.is_empty() {
            // サブタスクが空の場合
            let is_active = app.active_pane == Pane::Right;
            let fg_color = get_empty_view_color(is_active, colors);
            queue!(stdout, 
                cursor::MoveTo(split_x + 2, 1),
                SetForegroundColor(fg_color),
                Print("create (n)ew sub-task"),
                ResetColor
            )?;
        } else {
            // サブタスクが存在する場合
            let mut current_line = 0u16;
            let right_pane_width = width - split_x - 2;
            
            for (i, task) in subtasks.iter().enumerate() {
                if current_line >= height {
                    break;
                }
                
                let selected = app.active_pane == Pane::Right && i == app.right_pane_selected_index;
                let prefix = if i == app.right_pane_selected_index { "> " } else { "  " };
                let status = if task.completed { "[x]" } else { "[ ]" };
                
                let display = format!("{}{} {}", prefix, status, task.title);
                
                // 折り返し処理
                let max_width = right_pane_width as usize;
                let wrapped_lines = if display.width() > max_width {
                    wrap_text(&display, max_width)
                } else {
                    vec![display.clone()]
                };
                
                // 各行を描画
                for (line_idx, line) in wrapped_lines.iter().enumerate() {
                    if current_line >= height {
                        break;
                    }
                    
                    queue!(stdout, cursor::MoveTo(split_x + 2, current_line + 1))?;
                    
                    let is_first_line = line_idx == 0;
                    let display_text = if is_first_line {
                        line.clone()
                    } else {
                        // 2行目以降はインデント
                        format!("    {}", line)
                    };
                    
                    if selected {
                        let padding = " ".repeat(max_width.saturating_sub(display_text.width()));
                        queue!(stdout,
                            SetBackgroundColor(colors.selected_bg.to_crossterm_color()),
                            SetForegroundColor(colors.selected_fg.to_crossterm_color()),
                            Print(format!("{}{}", display_text, padding)),
                            ResetColor
                        )?;
                    } else {
                        // 通常表示（白色）
                        queue!(stdout, 
                            SetForegroundColor(Color::White),
                            Print(&display_text),
                            ResetColor
                        )?;
                    }
                    
                    current_line += 1;
                }
            }
        }
    } else {
        // 親タスクが空の場合
        let is_active = app.active_pane == Pane::Right;
        let fg_color = get_empty_view_color(is_active, colors);
        queue!(stdout, 
            cursor::MoveTo(split_x + 2, 1),
            SetForegroundColor(fg_color),
            Print("select task on left pane"),
            ResetColor
        )?;
    }
    
    Ok(())
}

fn draw_bottom_area<W: Write>(app: &App, stdout: &mut W, height: u16) -> io::Result<()> {
    let help_y = height.saturating_sub(2);
    let (width, _) = terminal::size()?;
    let colors = &app.settings.colors;
    
    queue!(stdout, cursor::MoveTo(0, help_y))?;
    execute!(stdout, terminal::Clear(ClearType::CurrentLine))?;
    
    if app.input_mode == InputMode::Adding {
        let task_type = if app.split_view && app.active_pane == Pane::Right {
            "New Sub-task"
        } else if app.current_parent.is_some() {
            "New Sub-task"
        } else {
            "New Task"
        };
        let message = format!("{} [Enter: Save, Esc: Cancel]", task_type);
        let padding_len = (width as usize).saturating_sub(message.len());
        
        // 入力バッファを画面幅に合わせて切り詰める
        let input_display = format!("> {}", app.input_buffer);
        let max_input_width = width as usize;
        let truncated_input = if input_display.width() > max_input_width {
            let mut current_width = "> ".width();
            let mut truncated = String::from("> ");
            
            for ch in app.input_buffer.chars() {
                let char_width = ch.width().unwrap_or(0);
                if current_width + char_width > max_input_width {
                    break;
                }
                truncated.push(ch);
                current_width += char_width;
            }
            truncated
        } else {
            input_display
        };
        
        queue!(
            stdout,
            cursor::MoveTo(0, help_y),
            SetBackgroundColor(colors.inactive_selected_bg.to_crossterm_color()),
            SetForegroundColor(colors.inactive_selected_fg.to_crossterm_color()),
            Print(format!("{}{}", message, " ".repeat(padding_len))),
            ResetColor,
            cursor::MoveTo(0, help_y + 1),
            Print(&truncated_input)
        )?;
    } else if app.input_mode == InputMode::Editing {
        // 入力バッファを画面幅に合わせて切り詰める
        let input_display = format!("> {}", app.input_buffer);
        let max_input_width = width as usize;
        let truncated_input = if input_display.width() > max_input_width {
            let mut current_width = "> ".width();
            let mut truncated = String::from("> ");
            
            for ch in app.input_buffer.chars() {
                let char_width = ch.width().unwrap_or(0);
                if current_width + char_width > max_input_width {
                    break;
                }
                truncated.push(ch);
                current_width += char_width;
            }
            truncated
        } else {
            input_display
        };
        
        queue!(
            stdout,
            cursor::MoveTo(0, help_y),
            SetBackgroundColor(colors.inactive_selected_bg.to_crossterm_color()),
            SetForegroundColor(colors.inactive_selected_fg.to_crossterm_color()),
            Print(format!("Edit Task [Enter: Save, Esc: Cancel]{}", " ".repeat((width as usize).saturating_sub(38)))),
            ResetColor,
            cursor::MoveTo(0, help_y + 1),
            Print(&truncated_input)
        )?;
    } else if app.input_mode == InputMode::Deleting {
        if let Some(task_id) = &app.deleting_task_id {
            if let Some(task) = app.tasks.iter().find(|t| t.id == *task_id) {
                let delete_text = format!("delete {}", task.title);
                
                // 画面幅に合わせて切り詰める
                let max_width = width as usize;
                let truncated_delete_text = if delete_text.width() > max_width {
                    let mut truncated = String::from("delete ");
                    let mut current_width = "delete ".width();
                    
                    for ch in task.title.chars() {
                        let char_width = ch.width().unwrap_or(0);
                        if current_width + char_width > max_width.saturating_sub(3) {
                            truncated.push_str("...");
                            break;
                        }
                        truncated.push(ch);
                        current_width += char_width;
                    }
                    truncated
                } else {
                    delete_text
                };
                
                let padding = " ".repeat(max_width.saturating_sub(truncated_delete_text.width()));
                queue!(
                    stdout,
                    cursor::MoveTo(0, help_y),
                    SetBackgroundColor(colors.delete_bg.to_crossterm_color()),
                    Print(format!("{}{}", truncated_delete_text, padding)),
                    ResetColor,
                    cursor::MoveTo(0, help_y + 1),
                    Print("> "),
                    SetForegroundColor(colors.delete_fg.to_crossterm_color()),
                    Print("(Y)es"),
                    ResetColor,
                    Print(" / (N)o")
                )?;
            }
        }
    } else if app.input_mode == InputMode::Help {
        // ヘルプモード時
        queue!(
            stdout,
            cursor::MoveTo(0, help_y),
            SetBackgroundColor(colors.inactive_selected_bg.to_crossterm_color()),
            SetForegroundColor(colors.inactive_selected_fg.to_crossterm_color()),
            Print(format!("Quick Help{}", " ".repeat((width as usize).saturating_sub(10)))),
            ResetColor,
            cursor::MoveTo(0, help_y + 1),
            Print("Press (m) to close help")
        )?;
    } else {
        let help_text = "(m)anual | (n)ew | [k]prev | [j]next | (e)dit | (d)el | [ ]finish! | [l]in | [h]out | [u/c/a]filter | [|]split | (q)uit";
        
        // 画面幅に合わせてヘルプテキストを切り詰める
        let max_help_width = width as usize;
        let truncated_help = if help_text.width() > max_help_width {
            // 表示幅を考慮して切り詰め
            let mut current_width = 0;
            let mut truncated = String::new();
            
            for ch in help_text.chars() {
                let char_width = ch.width().unwrap_or(0);
                if current_width + char_width > max_help_width.saturating_sub(3) {
                    truncated.push_str("...");
                    break;
                }
                truncated.push(ch);
                current_width += char_width;
            }
            truncated
        } else {
            help_text.to_string()
        };
        
        queue!(
            stdout,
            cursor::MoveTo(0, help_y),
            SetBackgroundColor(colors.inactive_selected_bg.to_crossterm_color()),
            SetForegroundColor(colors.inactive_selected_fg.to_crossterm_color()),
            Print(format!("Quick Help{}", " ".repeat((width as usize).saturating_sub(10)))),
            ResetColor,
            cursor::MoveTo(0, help_y + 1),
            Print(&truncated_help)
        )?;
    }
    
    Ok(())
}
