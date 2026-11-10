import type { ReactNode } from "react";
import { ActivityIndicator, View } from "react-native";
import { useColors } from "@/ui/theme";
import { useConnection } from "./ConnectionProvider";
import { ConnectScreen } from "./ConnectScreen";
import { WorkspacePicker } from "./WorkspacePicker";
export function WorkspaceGate({ children }: { children: ReactNode }) {
  const colors = useColors();
  const connection = useConnection();
  if (connection.restoring)
    return (
      <View
        style={{
          flex: 1,
          backgroundColor: colors.background,
          justifyContent: "center",
        }}
      >
        <ActivityIndicator color={colors.content} />
      </View>
    );
  if (!connection.pairing) return <ConnectScreen />;
  if (connection.selection === null) return <WorkspacePicker />;
  return children;
}
