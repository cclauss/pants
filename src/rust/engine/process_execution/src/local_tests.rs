use tempfile;
use testutil;

use crate::{
  CacheDest, CacheName, CommandRunner as CommandRunnerTrait, Context,
  FallibleProcessResultWithPlatform, NamedCaches, Platform, Process, RelativePath,
};
use hashing::EMPTY_DIGEST;
use shell_quote::bash;
use spectral::{assert_that, string::StrAssertions};
use std;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str;
use std::time::Duration;
use store::Store;
use tempfile::TempDir;
use testutil::data::{TestData, TestDirectory};
use testutil::path::{find_bash, which};
use testutil::{owned_string_vec, relative_paths};
use workunit_store::WorkunitStore;

#[derive(PartialEq, Debug)]
struct LocalTestResult {
  original: FallibleProcessResultWithPlatform,
  stdout_bytes: Vec<u8>,
  stderr_bytes: Vec<u8>,
}

#[tokio::test]
#[cfg(unix)]
async fn stdout() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(Process::new(owned_string_vec(&["/bin/echo", "-n", "foo"])))
    .await
    .unwrap();

  assert_eq!(result.stdout_bytes, "foo".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
}

#[tokio::test]
#[cfg(unix)]
async fn stdout_and_stderr_and_exit_code() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(Process::new(owned_string_vec(&[
    "/bin/bash",
    "-c",
    "echo -n foo ; echo >&2 -n bar ; exit 1",
  ])))
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "foo".as_bytes());
  assert_eq!(result.stderr_bytes, "bar".as_bytes());
  assert_eq!(result.original.exit_code, 1);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
}

#[tokio::test]
#[cfg(unix)]
async fn capture_exit_code_signal() {
  WorkunitStore::setup_for_tests();

  // Launch a process that kills itself with a signal.
  let result = run_command_locally(Process::new(owned_string_vec(&[
    "/bin/bash",
    "-c",
    "kill $$",
  ])))
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, -15);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
#[cfg(unix)]
async fn env() {
  WorkunitStore::setup_for_tests();

  let mut env: BTreeMap<String, String> = BTreeMap::new();
  env.insert("FOO".to_string(), "foo".to_string());
  env.insert("BAR".to_string(), "not foo".to_string());

  let result =
    run_command_locally(Process::new(owned_string_vec(&["/usr/bin/env"])).env(env.clone()))
      .await
      .unwrap();

  let stdout = String::from_utf8(result.stdout_bytes.to_vec()).unwrap();
  let got_env: BTreeMap<String, String> = stdout
    .split("\n")
    .filter(|line| !line.is_empty())
    .map(|line| line.splitn(2, "="))
    .map(|mut parts| {
      (
        parts.next().unwrap().to_string(),
        parts.next().unwrap_or("").to_string(),
      )
    })
    .filter(|x| x.0 != "PATH")
    .collect();

  assert_eq!(env, got_env);
}

#[tokio::test]
#[cfg(unix)]
async fn env_is_deterministic() {
  WorkunitStore::setup_for_tests();

  fn make_request() -> Process {
    let mut env = BTreeMap::new();
    env.insert("FOO".to_string(), "foo".to_string());
    env.insert("BAR".to_string(), "not foo".to_string());
    Process::new(owned_string_vec(&["/usr/bin/env"])).env(env)
  }

  let result1 = run_command_locally(make_request()).await;
  let result2 = run_command_locally(make_request()).await;

  assert_eq!(result1.unwrap(), result2.unwrap());
}

#[tokio::test]
async fn binary_not_found() {
  WorkunitStore::setup_for_tests();

  let err_string = run_command_locally(Process::new(owned_string_vec(&["echo", "-n", "foo"])))
    .await
    .expect_err("Want Err");
  assert!(err_string.contains("Failed to execute"));
  assert!(err_string.contains("echo"));
}

#[tokio::test]
async fn output_files_none() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(Process::new(owned_string_vec(&[
    &find_bash(),
    "-c",
    "exit 0",
  ])))
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
}

#[tokio::test]
async fn output_files_one() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!("echo -n {} > {}", TestData::roland().string(), "roland"),
    ])
    .output_files(relative_paths(&["roland"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::containing_roland().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn output_dirs() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!(
        "/bin/mkdir cats && echo -n {} > {} ; echo -n {} > treats",
        TestData::roland().string(),
        "cats/roland",
        TestData::catnip().string()
      ),
    ])
    .output_files(relative_paths(&["treats"]).collect())
    .output_directories(relative_paths(&["cats"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::recursive().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn output_files_many() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!(
        "echo -n {} > cats/roland ; echo -n {} > treats",
        TestData::roland().string(),
        TestData::catnip().string()
      ),
    ])
    .output_files(relative_paths(&["cats/roland", "treats"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::recursive().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn output_files_execution_failure() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!(
        "echo -n {} > {} ; exit 1",
        TestData::roland().string(),
        "roland"
      ),
    ])
    .output_files(relative_paths(&["roland"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 1);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::containing_roland().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn output_files_partial_output() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!("echo -n {} > {}", TestData::roland().string(), "roland"),
    ])
    .output_files(
      relative_paths(&["roland", "susannah"])
        .into_iter()
        .collect(),
    ),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::containing_roland().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn output_overlapping_file_and_dir() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!("echo -n {} > cats/roland", TestData::roland().string()),
    ])
    .output_files(relative_paths(&["cats/roland"]).collect())
    .output_directories(relative_paths(&["cats"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::nested().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn append_only_cache_created() {
  WorkunitStore::setup_for_tests();

  let name = "geo";
  let dest = format!(".cache/{}", name);
  let cache_name = CacheName::new(name.to_owned()).unwrap();
  let cache_dest = CacheDest::new(dest.clone()).unwrap();
  let result = run_command_locally(
    Process::new(vec!["/bin/ls".to_owned(), dest.clone()])
      .append_only_caches(vec![(cache_name, cache_dest)].into_iter().collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, format!("{}\n", dest).as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn jdk_symlink() {
  WorkunitStore::setup_for_tests();

  let preserved_work_tmpdir = TempDir::new().unwrap();
  let roland = TestData::roland().bytes();
  std::fs::write(preserved_work_tmpdir.path().join("roland"), roland.clone())
    .expect("Writing temporary file");

  let mut process = Process::new(vec!["/bin/cat".to_owned(), ".jdk/roland".to_owned()]);
  process.timeout = one_second();
  process.description = "cat roland".to_string();
  process.jdk_home = Some(preserved_work_tmpdir.path().to_path_buf());

  let result = run_command_locally(process).await.unwrap();

  assert_eq!(result.stdout_bytes, roland);
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn test_directory_preservation() {
  WorkunitStore::setup_for_tests();

  let preserved_work_tmpdir = TempDir::new().unwrap();
  let preserved_work_root = preserved_work_tmpdir.path().to_owned();

  let store_dir = TempDir::new().unwrap();
  let executor = task_executor::Executor::new();
  let store = Store::local_only(executor.clone(), store_dir.path()).unwrap();

  // Prepare the store to contain /cats/roland, because the EPR needs to materialize it and then run
  // from the ./cats directory.
  store
    .store_file_bytes(TestData::roland().bytes(), false)
    .await
    .expect("Error saving file bytes");
  store
    .record_directory(&TestDirectory::containing_roland().directory(), true)
    .await
    .expect("Error saving directory");
  store
    .record_directory(&TestDirectory::nested().directory(), true)
    .await
    .expect("Error saving directory");

  let cp = which("cp").expect("No cp on $PATH.");
  let bash_contents = format!("echo $PWD && {} roland ..", cp.display());
  let argv = vec![find_bash(), "-c".to_owned(), bash_contents.to_owned()];

  let mut process = Process::new(argv.clone()).output_files(relative_paths(&["roland"]).collect());
  process.input_files = TestDirectory::nested().digest();
  process.working_directory = Some(RelativePath::new("cats").unwrap());

  let result = run_command_locally_in_dir(
    process,
    preserved_work_root.clone(),
    false,
    Some(store),
    Some(executor),
  )
  .await;
  result.unwrap();

  assert!(preserved_work_root.exists());

  // Collect all of the top level sub-dirs under our test workdir.
  let subdirs = testutil::file::list_dir(&preserved_work_root);
  assert_eq!(subdirs.len(), 1);

  // Then look for a file like e.g. `/tmp/abc1234/process-execution7zt4pH/roland`
  let rolands_path = preserved_work_root.join(&subdirs[0]).join("roland");
  assert!(&rolands_path.exists());

  // Ensure that when a directory is preserved, a __run.sh file is created with the process's
  // command line and environment variables.
  let run_script_path = preserved_work_root.join(&subdirs[0]).join("__run.sh");
  assert!(&run_script_path.exists());

  std::fs::remove_file(&rolands_path).expect("Failed to remove roland.");

  // Confirm the script when run directly sets up the proper CWD.
  let mut child = std::process::Command::new(&run_script_path)
    .spawn()
    .expect("Failed to launch __run.sh");
  let status = child
    .wait()
    .expect("Failed to gather the result of __run.sh.");
  assert_eq!(Some(0), status.code());
  assert!(rolands_path.exists());

  // Ensure the bash command line is provided.
  let bytes_quoted_command_line = bash::escape(&bash_contents);
  let quoted_command_line = str::from_utf8(&bytes_quoted_command_line).unwrap();
  assert!(std::fs::read_to_string(&run_script_path)
    .unwrap()
    .contains(quoted_command_line));
}

#[tokio::test]
async fn test_directory_preservation_error() {
  WorkunitStore::setup_for_tests();

  let preserved_work_tmpdir = TempDir::new().unwrap();
  let preserved_work_root = preserved_work_tmpdir.path().to_owned();

  assert!(preserved_work_root.exists());
  assert_eq!(testutil::file::list_dir(&preserved_work_root).len(), 0);

  run_command_locally_in_dir(
    Process::new(vec!["doesnotexist".to_owned()]),
    preserved_work_root.clone(),
    false,
    None,
    None,
  )
  .await
  .expect_err("Want process to fail");

  assert!(preserved_work_root.exists());
  // Collect all of the top level sub-dirs under our test workdir.
  assert_eq!(testutil::file::list_dir(&preserved_work_root).len(), 1);
}

#[tokio::test]
async fn all_containing_directories_for_outputs_are_created() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      format!(
        // mkdir would normally fail, since birds/ doesn't yet exist, as would echo, since cats/
        // does not exist, but we create the containing directories for all outputs before the
        // process executes.
        "/bin/mkdir birds/falcons && echo -n {} > cats/roland",
        TestData::roland().string()
      ),
    ])
    .output_files(relative_paths(&["cats/roland"]).collect())
    .output_directories(relative_paths(&["birds/falcons"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::nested_dir_and_file().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn output_empty_dir() {
  WorkunitStore::setup_for_tests();

  let result = run_command_locally(
    Process::new(vec![
      find_bash(),
      "-c".to_owned(),
      "/bin/mkdir falcons".to_string(),
    ])
    .output_directories(relative_paths(&["falcons"]).collect()),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(
    result.original.output_directory,
    TestDirectory::containing_falcons_dir().digest()
  );
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

#[tokio::test]
async fn timeout() {
  WorkunitStore::setup_for_tests();

  let argv = vec![
    find_bash(),
    "-c".to_owned(),
    "/bin/sleep 0.2; /bin/echo -n 'European Burmese'".to_string(),
  ];

  let mut process = Process::new(argv);
  process.timeout = Some(Duration::from_millis(100));
  process.description = "sleepy-cat".to_string();

  let result = run_command_locally(process).await.unwrap();

  assert_eq!(result.original.exit_code, -15);
  let error_msg = String::from_utf8(result.stdout_bytes.to_vec()).unwrap();
  assert_that(&error_msg).contains("Exceeded timeout");
  assert_that(&error_msg).contains("sleepy-cat");
}

#[tokio::test]
async fn working_directory() {
  WorkunitStore::setup_for_tests();

  let store_dir = TempDir::new().unwrap();
  let executor = task_executor::Executor::new();
  let store = Store::local_only(executor.clone(), store_dir.path()).unwrap();

  // Prepare the store to contain /cats/roland, because the EPR needs to materialize it and then run
  // from the ./cats directory.
  store
    .store_file_bytes(TestData::roland().bytes(), false)
    .await
    .expect("Error saving file bytes");
  store
    .record_directory(&TestDirectory::containing_roland().directory(), true)
    .await
    .expect("Error saving directory");
  store
    .record_directory(&TestDirectory::nested().directory(), true)
    .await
    .expect("Error saving directory");

  let work_dir = TempDir::new().unwrap();

  let mut process = Process::new(vec![find_bash(), "-c".to_owned(), "/bin/ls".to_string()]);
  process.working_directory = Some(RelativePath::new("cats").unwrap());
  process.input_files = TestDirectory::nested().digest();
  process.timeout = one_second();
  process.description = "confused-cat".to_string();

  let result = run_command_locally_in_dir(
    process,
    work_dir.path().to_owned(),
    true,
    Some(store),
    Some(executor),
  )
  .await
  .unwrap();

  assert_eq!(result.stdout_bytes, "roland\n".as_bytes());
  assert_eq!(result.stderr_bytes, "".as_bytes());
  assert_eq!(result.original.exit_code, 0);
  assert_eq!(result.original.output_directory, EMPTY_DIGEST);
  assert_eq!(result.original.platform, Platform::current().unwrap());
}

async fn run_command_locally(req: Process) -> Result<LocalTestResult, String> {
  let work_dir = TempDir::new().unwrap();
  let work_dir_path = work_dir.path().to_owned();
  run_command_locally_in_dir(req, work_dir_path, true, None, None).await
}

async fn run_command_locally_in_dir(
  req: Process,
  dir: PathBuf,
  cleanup: bool,
  store: Option<Store>,
  executor: Option<task_executor::Executor>,
) -> Result<LocalTestResult, String> {
  let store_dir = TempDir::new().unwrap();
  let named_cache_dir = TempDir::new().unwrap();
  let executor = executor.unwrap_or_else(|| task_executor::Executor::new());
  let store =
    store.unwrap_or_else(|| Store::local_only(executor.clone(), store_dir.path()).unwrap());
  let runner = crate::local::CommandRunner::new(
    store.clone(),
    executor.clone(),
    dir,
    NamedCaches::new(named_cache_dir.path().to_owned()),
    cleanup,
  );
  let original = runner.run(req.into(), Context::default()).await?;
  let stdout_bytes: Vec<u8> = store
    .load_file_bytes_with(original.stdout_digest, |bytes| bytes.into())
    .await?
    .unwrap()
    .0;
  let stderr_bytes: Vec<u8> = store
    .load_file_bytes_with(original.stderr_digest, |bytes| bytes.into())
    .await?
    .unwrap()
    .0;
  Ok(LocalTestResult {
    original,
    stdout_bytes,
    stderr_bytes,
  })
}

fn one_second() -> Option<Duration> {
  Some(Duration::from_millis(1000))
}
