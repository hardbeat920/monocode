# Use multiple local folders in one conversation / 한 대화에서 여러 로컬 폴더 사용하기

## English

Open **Additional folders** from the project menu in the native app. Select **Add folder** and choose a folder to connect. Select **Done** to close the dialog. Connected folders appear with their full paths. Use **Remove** to disconnect a folder. The list is saved per project and persists across app restarts.

Additional folders require a local Codex conversation and Codex CLI 0.167.1 or later. Other providers and remote projects do not receive additional folders. Starting an unsupported provider in a project with connected folders shows an error with instructions to remove them. Older Codex versions can ignore folder access settings, so the app blocks execution for projects using this feature and asks you to update.

Changes apply to the next regular turn after the current execution finishes. Messages sent while a turn is running use that turn's folder list. You can continue using an existing conversation.

The primary working folder, file explorer, and Git root stay the same. In modes that allow edits, connected folders are included as writable roots. Read-only and plan modes still restrict writes. Approval and network policies still apply.

The folder list does not restrict file reads to these paths. Removing a folder does not erase earlier conversation content. It does not revoke access granted by other means, such as full-access mode or a path under the primary working folder.

If a connected folder disappears or points to a different target, execution stops with an error. Remove that folder from the list, then connect it again.

## 한국어

네이티브 앱의 프로젝트 메뉴에서 **Additional folders**를 엽니다. **Add folder**로 연결할 폴더를 선택하고 **Done**으로 닫습니다. 연결된 폴더는 전체 경로로 표시되며 **Remove**로 제거합니다. 목록은 프로젝트별로 저장되어 앱을 다시 열어도 유지됩니다.

로컬 Codex 대화와 Codex CLI 0.167.1 이상에서 지원합니다. 다른 제공자와 원격 프로젝트에는 추가 폴더를 전달하지 않습니다. 연결된 폴더가 있는 프로젝트에서 미지원 제공자를 실행하면 오류와 해결 방법을 표시합니다. 구버전 Codex는 폴더 접근 설정을 무시할 수 있으므로 기능을 사용한 프로젝트의 실행을 차단하고 업데이트를 안내합니다.

변경은 현재 실행이 끝난 뒤 시작하는 일반 턴부터 적용됩니다. 실행 중 추가로 보낸 메시지는 현재 턴의 폴더 목록을 사용합니다. 기존 대화를 새로 만들 필요는 없습니다.

기본 작업 폴더와 파일 탐색기·Git의 기준은 유지됩니다. 수정 가능한 모드에서는 연결된 폴더도 쓰기 대상으로 포함합니다. 읽기 전용·계획 모드는 계속 쓰기를 제한하며 승인과 네트워크 정책도 유지합니다.

폴더 목록은 파일 읽기를 이 경로로만 제한하는 설정이 아닙니다. 폴더를 제거해도 이전 대화 내용은 남습니다. 전체 접근 모드나 기본 작업 폴더 하위 경로 등 다른 근거로 허용된 접근까지 취소하지는 않습니다.

연결된 폴더가 사라지거나 다른 대상으로 바뀌면 실행을 중단하고 오류를 표시합니다. 목록에서 해당 폴더를 제거한 뒤 다시 연결합니다.
