# TerDO -Terminal ToDO-

コマンドプロンプト（Windowsターミナル）上で動くTODOアプリケーション。
現在，Windows11以外のことは考えずに作成しています。

## 使用言語

- Rust

## 実行方法

1. リポジトリをクローン
2. cd terdo
3. cargo build（or cargo build release）
4. cargo run (or Run to terdo.exe)

## 使い方

- q: (q)uit
- n: create (n)ew task
- k,↑: go to prev task
- j,↓: go to next task
- e: (e)dit task
- d: (d)elete task
- space: toggle completed
- enter,l,→: into the list of subtask
- backspace,h,←: exit the list of subtask
- u/c/a: select a filter type (unfinished/completed/all)
- |: split pain

## setting.tomlで設定できる項目

現在は色情報しか設定できません。->申し訳程度にペイン分割状態を保持するようにしました。

```setting.toml
split_view # ペイン分割の際に状態の変更を保持
[colors.selected_bg] # 選択中のタスクの背景色(RGB)
[colors.selected_fg] # 選択中のタスクの文字色(RGB)
[colors.inactive_selected_bg] # アクティブじゃない状態の背景色(RGB)
[colors.inactive_selected_fg] # アクティブじゃない状態の文字色(RGB)
[colors.delete_bg] # タスクの削除時の背景色(RGB)
[colors.delete_fg] # タスクの削除時の文字色(RGB)
[colors.title_fg] # タイトルの文字色(RGB)
[colors.filter_all_fg] # 全選択モード用ステータス文字色(RGB)
[colors.filter_completed_fg] # 完了済み表示モード用ステータス文字色(RGB)
[colors.filter_unfinished_fg] # 未完了表示モード用ステータス文字色(RGB)
[colors.empty_view_fg] # タスクがないときに表示するメッセージ文字色(RGB)
```
