import { Redirect, router } from "expo-router";
import { Pressable, Text, View } from "react-native";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { SessionScreen } from "@/features/sessions/SessionScreen";
import { useConnectionStyles } from "@/features/connection/ConnectScreen";
export default function Index() {
  const styles = useConnectionStyles();
  const { projects, monos } = useWorkspace();
  if (projects[0])
    return (
      <Redirect
        href={{
          pathname: "/project/[projectId]",
          params: { projectId: projects[0].id },
        }}
      />
    );
  if (monos[0])
    return (
      <Redirect
        href={{ pathname: "/mono/[monoId]", params: { monoId: monos[0].id } }}
      />
    );
  return (
    <SessionScreen title="Workspace">
      <View style={[styles.screen, { justifyContent: "center", gap: 24 }]}>
        <Text style={styles.title}>Make yourself at home.</Text>
        <Text style={styles.subtitle}>
          Choose the projects and monos you want on your phone.
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
