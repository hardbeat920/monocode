import { Redirect, router } from "expo-router";
import { useConnection } from "@/features/connection/ConnectionProvider";
import { homeProjectId, readLastPlace } from "@/features/workspace/lastPlace";
import { Pressable, Text, View } from "react-native";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { SessionScreen } from "@/features/sessions/SessionScreen";
import { useConnectionStyles } from "@/features/connection/ConnectScreen";
export default function Index() {
  const styles = useConnectionStyles();
  const { projects, monos } = useWorkspace();
  // Always a project, never a Mono: back where the user last was while it is
  // still on the phone, else the first.
  const home = homeProjectId(
    projects,
    monos,
    readLastPlace(useConnection().pairing?.desktop.fingerprint),
  );
  if (home)
    return (
      <Redirect href={{ pathname: "/project/[projectId]", params: { projectId: home } }} />
    );
  return (
    <SessionScreen title="Workspace">
      <View style={[styles.screen, { justifyContent: "center", gap: 24 }]}>
        <Text style={styles.title}>Make yourself at home.</Text>
        <Text style={styles.subtitle}>
          Choose the projects you want on your phone.
        </Text>
        <Pressable
          style={styles.primary}
          onPress={() => router.push("/settings")}
        >
          <Text style={styles.primaryLabel}>Choose workspace</Text>
        </Pressable>
      </View>
    </SessionScreen>
  );
}
