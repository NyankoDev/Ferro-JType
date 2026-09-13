import java.net.URI;
import java.nio.file.FileSystems;
import java.nio.file.Files;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;

class ExportJrt {
    public static void main(String[] args) throws Exception {
        var modules = FileSystems.getFileSystem(URI.create("jrt:/")).getPath("/modules");
        try (var paths = Files.walk(modules);
             var output = new ZipOutputStream(System.out)) {
            var classes = paths.filter(path -> path.toString().endsWith(".class"))
                .filter(path -> !path.getFileName().toString().equals("module-info.class"))
                .sorted().iterator();
            while (classes.hasNext()) {
                var path = classes.next();
                output.putNextEntry(new ZipEntry(modules.relativize(path).toString()));
                Files.copy(path, output);
                output.closeEntry();
            }
        }
    }
}
