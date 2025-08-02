package co.juandevgo.paleoapi.domain;

import lombok.AllArgsConstructor;
import lombok.Data;
import lombok.NonNull;

import java.util.List;

@Data
@AllArgsConstructor
public class Specie {

    @NonNull
    private Long id;

    @NonNull
    private String name;

    @NonNull
    private String group;

    private String phonetic;

    @NonNull
    private String diet;

    @NonNull
    private String size;

    @NonNull
    private String timePeriod;

    @NonNull
    private List<String> locations;

    @NonNull
    private List<String> classification;

    private List<String> images;
}