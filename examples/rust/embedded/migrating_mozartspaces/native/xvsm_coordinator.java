// MozartSpaces XVSM Coordinator (Java)
// Demonstrates eXtended Virtual Shared Memory with coordinator objects

import java.util.List;
import java.util.ArrayList;

public class XVSMCoordinator {
    private List<Tuple> fifoQueue = new ArrayList<>();
    private List<Tuple> lifoStack = new ArrayList<>();
    private Map<String, List<Tuple>> labeledTuples = new HashMap<>();
    
    // FIFO coordinator
    public void writeFIFO(Tuple tuple) {
        fifoQueue.add(tuple);
    }
    
    public Tuple takeFIFO() {
        return fifoQueue.isEmpty() ? null : fifoQueue.remove(0);
    }
    
    // LIFO coordinator
    public void writeLIFO(Tuple tuple) {
        lifoStack.add(tuple);
    }
    
    public Tuple takeLIFO() {
        return lifoStack.isEmpty() ? null : lifoStack.remove(lifoStack.size() - 1);
    }
    
    // Label-based coordinator
    public void writeLabel(String label, Tuple tuple) {
        labeledTuples.computeIfAbsent(label, k -> new ArrayList<>()).add(tuple);
    }
    
    public Tuple readLabel(String label) {
        List<Tuple> tuples = labeledTuples.get(label);
        return tuples == null || tuples.isEmpty() ? null : tuples.get(0);
    }
}

// Usage:
// XVSMCoordinator coordinator = new XVSMCoordinator();
// coordinator.writeFIFO(new Tuple("item", "data"));
// Tuple result = coordinator.takeFIFO();
